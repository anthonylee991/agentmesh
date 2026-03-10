"""AgentMesh Python client — connect any Python agent to the mesh."""

from __future__ import annotations

import asyncio
import json
import uuid
from typing import Any, Callable, Optional

import websockets
from websockets.asyncio.client import ClientConnection


class AgentMeshClient:
    """WebSocket client for AgentMesh broker.

    Usage:
        client = AgentMeshClient("ws://localhost:7777/ws")
        await client.connect()
        await client.register("my-agent", "my-project", capabilities=["domain_expert"])

        # Ask another agent/project a question
        response = await client.ask("other-project", "What is the API schema?")

        # Listen for incoming messages
        async for message in client.listen():
            print(f"Got: {message}")

        await client.disconnect()
    """

    def __init__(self, broker_url: str = "ws://localhost:7777/ws"):
        self.broker_url = broker_url
        self._ws: Optional[ClientConnection] = None
        self._agent_id: Optional[str] = None
        self._inbox: list[dict] = []
        self._pending: dict[str, asyncio.Future] = {}
        self._listener_task: Optional[asyncio.Task] = None
        self._on_message: Optional[Callable] = None

    @property
    def agent_id(self) -> Optional[str]:
        return self._agent_id

    @property
    def connected(self) -> bool:
        return self._ws is not None

    async def connect(self) -> None:
        """Connect to the AgentMesh broker."""
        self._ws = await websockets.connect(self.broker_url)
        self._listener_task = asyncio.create_task(self._read_loop())

    async def disconnect(self) -> None:
        """Deregister and disconnect."""
        if self._ws:
            await self._send_op("deregister")
            if self._listener_task:
                self._listener_task.cancel()
            await self._ws.close()
            self._ws = None
            self._agent_id = None

    async def register(
        self,
        name: str,
        project: str,
        project_path: Optional[str] = None,
        platform: str = "custom",
        capabilities: Optional[list[str]] = None,
    ) -> str:
        """Register this agent on the mesh. Returns the assigned agent_id."""
        payload = {
            "name": name,
            "project": project,
            "platform": platform,
            "capabilities": capabilities or [],
        }
        if project_path:
            payload["project_path"] = project_path

        response = await self._send_and_wait("register", payload)
        self._agent_id = response.get("agent_id", "")
        return self._agent_id

    async def discover(
        self,
        project: Optional[str] = None,
        capability: Optional[str] = None,
        platform: Optional[str] = None,
        online_only: bool = True,
    ) -> list[dict]:
        """Discover agents on the mesh."""
        payload: dict[str, Any] = {"online_only": online_only}
        if project:
            payload["project"] = project
        if capability:
            payload["capability"] = capability
        if platform:
            payload["platform"] = platform

        response = await self._send_and_wait("discover", payload)
        return response.get("agents", [])

    async def ask(
        self,
        to: str,
        question: str,
        project: Optional[str] = None,
        timeout: float = 60.0,
    ) -> dict:
        """Send an ask message and wait for a response."""
        msg_id = str(uuid.uuid4())
        message = {
            "id": msg_id,
            "from": self._agent_id or "unknown",
            "to": to,
            "msg_type": "ask",
            "content": {"text": question, "data": None, "attachments": []},
            "correlation_id": None,
            "timestamp": _now_iso(),
            "ttl": 300,
            "proxy_response": False,
        }
        if project:
            message["project"] = project

        future: asyncio.Future = asyncio.get_event_loop().create_future()
        self._pending[msg_id] = future

        await self._send_op("send", message)

        try:
            result = await asyncio.wait_for(future, timeout=timeout)
            return result
        except asyncio.TimeoutError:
            self._pending.pop(msg_id, None)
            raise TimeoutError(f"No response within {timeout}s")

    async def respond(self, to_message: dict, text: str) -> None:
        """Send a response to an incoming ask message."""
        response = {
            "id": str(uuid.uuid4()),
            "from": self._agent_id or "unknown",
            "to": to_message.get("from", ""),
            "msg_type": "response",
            "content": {"text": text, "data": None, "attachments": []},
            "correlation_id": to_message.get("id"),
            "timestamp": _now_iso(),
            "ttl": 300,
            "proxy_response": False,
        }
        await self._send_op("send", response)

    async def check_messages(self) -> list[dict]:
        """Return and clear the inbox of received messages."""
        messages = list(self._inbox)
        self._inbox.clear()
        return messages

    async def status(self) -> dict:
        """Get broker status."""
        return await self._send_and_wait("status", None)

    def on_message(self, callback: Callable) -> None:
        """Set a callback for incoming messages.
        Callback signature: async def handler(message: dict) -> None"""
        self._on_message = callback

    # --- Internal ---

    async def _send_op(self, op: str, payload: Any = None) -> None:
        if not self._ws:
            raise RuntimeError("Not connected")
        msg: dict[str, Any] = {"op": op}
        if payload is not None:
            msg["payload"] = payload
        await self._ws.send(json.dumps(msg))

    async def _send_and_wait(self, op: str, payload: Any, timeout: float = 10.0) -> dict:
        future: asyncio.Future = asyncio.get_event_loop().create_future()
        self._pending[f"_op_{op}"] = future
        await self._send_op(op, payload)
        try:
            return await asyncio.wait_for(future, timeout=timeout)
        except asyncio.TimeoutError:
            self._pending.pop(f"_op_{op}", None)
            raise TimeoutError(f"No response for '{op}' within {timeout}s")

    async def _read_loop(self) -> None:
        if not self._ws:
            return
        try:
            async for raw in self._ws:
                try:
                    msg = json.loads(raw)
                except json.JSONDecodeError:
                    continue

                op = msg.get("op", "")
                payload = msg.get("payload", {})

                # Route control responses to pending futures
                if op == "registered":
                    self._resolve(f"_op_register", payload)
                elif op == "discover_result":
                    self._resolve(f"_op_discover", payload)
                elif op == "status_result":
                    self._resolve(f"_op_status", payload)
                elif op == "error":
                    self._resolve_error(payload)
                elif op == "deliver":
                    await self._handle_deliver(payload)
        except websockets.ConnectionClosed:
            pass

    async def _handle_deliver(self, message: dict) -> None:
        msg_type = message.get("msg_type", "")
        correlation_id = message.get("correlation_id")

        # If this is a response to a pending ask, resolve the future
        if msg_type == "response" and correlation_id and correlation_id in self._pending:
            self._resolve(correlation_id, message)
        else:
            # Incoming ask or broadcast — add to inbox
            self._inbox.append(message)
            if self._on_message:
                await self._on_message(message)

    def _resolve(self, key: str, value: dict) -> None:
        future = self._pending.pop(key, None)
        if future and not future.done():
            future.set_result(value)

    def _resolve_error(self, payload: dict) -> None:
        # Try to resolve any pending operation with the error
        for key, future in list(self._pending.items()):
            if key.startswith("_op_") and not future.done():
                future.set_exception(
                    RuntimeError(payload.get("message", "Unknown error"))
                )
                self._pending.pop(key, None)
                return


def _now_iso() -> str:
    from datetime import datetime, timezone
    return datetime.now(timezone.utc).isoformat()
