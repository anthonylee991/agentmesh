/**
 * AgentMesh Node.js client — connect any JS/TS agent to the mesh.
 *
 * Usage:
 *   const client = new AgentMeshClient();
 *   await client.connect();
 *   await client.register("my-agent", "my-project", { capabilities: ["domain_expert"] });
 *
 *   const response = await client.ask("other-project", "What is the API schema?");
 *   console.log(response.content.text);
 *
 *   client.onMessage((msg) => console.log("Incoming:", msg));
 *
 *   await client.disconnect();
 */

import WebSocket from "ws";
import { randomUUID } from "crypto";

export interface AgentMeshOptions {
  brokerUrl?: string;
}

export interface MeshMessage {
  id: string;
  from: string;
  to: string;
  msg_type: "ask" | "response" | "broadcast" | "system";
  content: { text: string; data?: unknown; attachments?: string[] };
  project?: string;
  correlation_id?: string;
  timestamp: string;
  ttl: number;
  proxy_response: boolean;
}

export interface AgentInfo {
  agent_id: string;
  name: string;
  project: string;
  platform?: string;
  capabilities: string[];
  status: string;
}

type PendingResolve = {
  resolve: (value: Record<string, unknown>) => void;
  reject: (error: Error) => void;
};

export class AgentMeshClient {
  private brokerUrl: string;
  private ws: WebSocket | null = null;
  private agentId: string | null = null;
  private inbox: MeshMessage[] = [];
  private pending = new Map<string, PendingResolve>();
  private messageHandler: ((msg: MeshMessage) => void) | null = null;

  constructor(options: AgentMeshOptions = {}) {
    this.brokerUrl = options.brokerUrl || "ws://localhost:7777/ws";
  }

  get id(): string | null {
    return this.agentId;
  }

  get connected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  /** Connect to the AgentMesh broker. */
  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(this.brokerUrl);

      this.ws.on("open", () => resolve());
      this.ws.on("error", (err) => reject(err));
      this.ws.on("close", () => {
        this.ws = null;
        this.agentId = null;
      });

      this.ws.on("message", (data) => {
        try {
          const msg = JSON.parse(data.toString());
          this.handleMessage(msg);
        } catch {
          // Ignore invalid JSON
        }
      });
    });
  }

  /** Disconnect from the broker. */
  async disconnect(): Promise<void> {
    if (this.ws) {
      this.sendOp("deregister");
      await new Promise<void>((resolve) => {
        setTimeout(() => {
          this.ws?.close();
          resolve();
        }, 100);
      });
    }
  }

  /** Register this agent on the mesh. */
  async register(
    name: string,
    project: string,
    options: {
      projectPath?: string;
      platform?: string;
      capabilities?: string[];
    } = {}
  ): Promise<string> {
    const payload: Record<string, unknown> = {
      name,
      project,
      platform: options.platform || "custom",
      capabilities: options.capabilities || [],
    };
    if (options.projectPath) {
      payload.project_path = options.projectPath;
    }

    const response = await this.sendAndWait("register", payload);
    this.agentId = (response as { agent_id?: string }).agent_id || "";
    return this.agentId;
  }

  /** Discover agents on the mesh. */
  async discover(options: {
    project?: string;
    capability?: string;
    platform?: string;
    onlineOnly?: boolean;
  } = {}): Promise<AgentInfo[]> {
    const payload: Record<string, unknown> = {
      online_only: options.onlineOnly ?? true,
    };
    if (options.project) payload.project = options.project;
    if (options.capability) payload.capability = options.capability;
    if (options.platform) payload.platform = options.platform;

    const response = await this.sendAndWait("discover", payload);
    return (response as { agents?: AgentInfo[] }).agents || [];
  }

  /** Ask another agent/project a question and wait for a response. */
  async ask(
    to: string,
    question: string,
    options: { project?: string; timeout?: number } = {}
  ): Promise<MeshMessage> {
    const msgId = randomUUID();
    const message: MeshMessage = {
      id: msgId,
      from: this.agentId || "unknown",
      to,
      msg_type: "ask",
      content: { text: question, attachments: [] },
      timestamp: new Date().toISOString(),
      ttl: 300,
      proxy_response: false,
    };
    if (options.project) {
      message.project = options.project;
    }

    const timeout = options.timeout || 60_000;

    const promise = new Promise<Record<string, unknown>>((resolve, reject) => {
      this.pending.set(msgId, { resolve, reject });
      setTimeout(() => {
        if (this.pending.has(msgId)) {
          this.pending.delete(msgId);
          reject(new Error(`No response within ${timeout}ms`));
        }
      }, timeout);
    });

    this.sendOp("send", message);
    return (await promise) as unknown as MeshMessage;
  }

  /** Respond to an incoming ask message. */
  respond(toMessage: MeshMessage, text: string): void {
    const response: MeshMessage = {
      id: randomUUID(),
      from: this.agentId || "unknown",
      to: toMessage.from,
      msg_type: "response",
      content: { text, attachments: [] },
      correlation_id: toMessage.id,
      timestamp: new Date().toISOString(),
      ttl: 300,
      proxy_response: false,
    };
    this.sendOp("send", response);
  }

  /** Check and clear the inbox. */
  checkMessages(): MeshMessage[] {
    const messages = [...this.inbox];
    this.inbox = [];
    return messages;
  }

  /** Get broker status. */
  async status(): Promise<Record<string, unknown>> {
    return this.sendAndWait("status", undefined);
  }

  /** Set a handler for incoming messages (asks, broadcasts). */
  onMessage(handler: (msg: MeshMessage) => void): void {
    this.messageHandler = handler;
  }

  // --- Internal ---

  private sendOp(op: string, payload?: unknown): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error("Not connected");
    }
    const msg: Record<string, unknown> = { op };
    if (payload !== undefined) {
      msg.payload = payload;
    }
    this.ws.send(JSON.stringify(msg));
  }

  private sendAndWait(
    op: string,
    payload: unknown,
    timeout = 10_000
  ): Promise<Record<string, unknown>> {
    return new Promise((resolve, reject) => {
      const key = `_op_${op}`;
      this.pending.set(key, { resolve, reject });
      this.sendOp(op, payload);
      setTimeout(() => {
        if (this.pending.has(key)) {
          this.pending.delete(key);
          reject(new Error(`No response for '${op}' within ${timeout}ms`));
        }
      }, timeout);
    });
  }

  private handleMessage(msg: { op: string; payload?: Record<string, unknown> }): void {
    const { op, payload } = msg;

    switch (op) {
      case "registered":
        this.resolvePending("_op_register", payload || {});
        break;
      case "discover_result":
        this.resolvePending("_op_discover", payload || {});
        break;
      case "status_result":
        this.resolvePending("_op_status", payload || {});
        break;
      case "error":
        this.resolveError(payload || {});
        break;
      case "deliver":
        this.handleDeliver(payload as unknown as MeshMessage);
        break;
    }
  }

  private handleDeliver(message: MeshMessage): void {
    if (
      message.msg_type === "response" &&
      message.correlation_id &&
      this.pending.has(message.correlation_id)
    ) {
      this.resolvePending(
        message.correlation_id,
        message as unknown as Record<string, unknown>
      );
    } else {
      this.inbox.push(message);
      this.messageHandler?.(message);
    }
  }

  private resolvePending(key: string, value: Record<string, unknown>): void {
    const entry = this.pending.get(key);
    if (entry) {
      this.pending.delete(key);
      entry.resolve(value);
    }
  }

  private resolveError(payload: Record<string, unknown>): void {
    for (const [key, entry] of this.pending) {
      if (key.startsWith("_op_")) {
        this.pending.delete(key);
        entry.reject(
          new Error((payload.message as string) || "Unknown error")
        );
        return;
      }
    }
  }
}
