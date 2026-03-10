import { WebSocketServer, WebSocket } from "ws";
import type { Server } from "http";
import { db } from "../db/index.js";
import { agents, relayMessages } from "../db/schema.js";
import { eq, and, or } from "drizzle-orm";

/** Connected WebSocket clients, keyed by agent_id */
const connections = new Map<
  string,
  { ws: WebSocket; licenseKey: string; agentId: string }
>();

/** Set up the WebSocket relay on the HTTP server.
 *  Clients connect to: ws://host/mesh/ws?agent_id=X&license_key=Y */
export function setupWebSocketRelay(server: Server) {
  const wss = new WebSocketServer({ server, path: "/mesh/ws" });

  wss.on("connection", async (ws, req) => {
    const url = new URL(req.url || "/", `http://${req.headers.host}`);
    const agentId = url.searchParams.get("agent_id");
    const licenseKey = url.searchParams.get("license_key");

    if (!agentId || !licenseKey) {
      ws.close(4001, "Missing agent_id or license_key");
      return;
    }

    // Validate license exists
    const [user] = await db.query.users.findMany({
      where: (u, { eq }) => eq(u.licenseKey, licenseKey),
      limit: 1,
    });

    if (!user) {
      ws.close(4003, "Invalid license key");
      return;
    }

    // Mark agent online
    await db
      .update(agents)
      .set({ online: true, lastSeen: new Date() })
      .where(eq(agents.agentId, agentId));

    // Store connection
    connections.set(agentId, { ws, licenseKey, agentId });
    console.log(
      `[relay] Agent connected: ${agentId} (license: ${licenseKey.slice(0, 8)}...)`
    );

    // Send ack
    ws.send(JSON.stringify({ type: "connected", agent_id: agentId }));

    // Deliver any pending messages
    await deliverPending(agentId, licenseKey, ws);

    // Handle incoming messages
    ws.on("message", async (data) => {
      try {
        const msg = JSON.parse(data.toString());
        await handleRelayMessage(agentId, licenseKey, msg);
      } catch (e) {
        ws.send(
          JSON.stringify({
            type: "error",
            message: `Invalid message: ${e}`,
          })
        );
      }
    });

    // Handle heartbeat pings
    ws.on("pong", () => {
      db.update(agents)
        .set({ lastSeen: new Date() })
        .where(eq(agents.agentId, agentId))
        .then(() => {});
    });

    // Cleanup on disconnect
    ws.on("close", async () => {
      connections.delete(agentId);
      await db
        .update(agents)
        .set({ online: false })
        .where(eq(agents.agentId, agentId));
      console.log(`[relay] Agent disconnected: ${agentId}`);
    });
  });

  // Heartbeat interval — ping all clients every 30s
  setInterval(() => {
    for (const [, conn] of connections) {
      if (conn.ws.readyState === WebSocket.OPEN) {
        conn.ws.ping();
      }
    }
  }, 30_000);

  // Prune old messages every 6 hours
  setInterval(
    async () => {
      try {
        const cutoff = new Date(Date.now() - 7 * 24 * 60 * 60 * 1000); // 7 days
        const result = await db.execute(
          `DELETE FROM relay_messages WHERE created_at < '${cutoff.toISOString()}'`
        );
        console.log(`[relay] Pruned old messages`);
      } catch (e) {
        console.error("[relay] Prune error:", e);
      }
    },
    6 * 60 * 60 * 1000
  );

  console.log("[relay] WebSocket relay ready");
}

/** Handle a message from a connected agent */
async function handleRelayMessage(
  fromAgentId: string,
  licenseKey: string,
  msg: Record<string, unknown>
) {
  const type = msg.type as string;

  switch (type) {
    case "relay": {
      // Relay a message to another agent
      const toAgentId = msg.to_agent_id as string | undefined;
      const toProject = msg.to_project as string | undefined;
      const payload = msg.payload as string; // Encrypted blob

      if (!payload) {
        const conn = connections.get(fromAgentId);
        conn?.ws.send(
          JSON.stringify({ type: "error", message: "Missing payload" })
        );
        return;
      }

      // Try direct delivery first
      if (toAgentId) {
        const target = connections.get(toAgentId);
        if (target && target.licenseKey === licenseKey) {
          target.ws.send(
            JSON.stringify({
              type: "deliver",
              from_agent_id: fromAgentId,
              payload,
            })
          );
          return;
        }
      }

      // Try project-based delivery
      if (toProject) {
        let delivered = false;
        for (const [, conn] of connections) {
          if (conn.licenseKey !== licenseKey || conn.agentId === fromAgentId)
            continue;
          // Check if this agent belongs to the target project
          const [agent] = await db
            .select()
            .from(agents)
            .where(
              and(eq(agents.agentId, conn.agentId), eq(agents.project, toProject))
            )
            .limit(1);
          if (agent) {
            conn.ws.send(
              JSON.stringify({
                type: "deliver",
                from_agent_id: fromAgentId,
                payload,
              })
            );
            delivered = true;
            break;
          }
        }
        if (delivered) return;
      }

      // Store for later delivery (store-and-forward)
      await db.insert(relayMessages).values({
        fromAgentId,
        toAgentId: toAgentId || null,
        toProject: toProject || null,
        licenseKey,
        payload,
      });

      const sender = connections.get(fromAgentId);
      sender?.ws.send(
        JSON.stringify({ type: "queued", message: "Message stored for delivery" })
      );
      break;
    }

    case "heartbeat": {
      await db
        .update(agents)
        .set({ lastSeen: new Date() })
        .where(eq(agents.agentId, fromAgentId));
      break;
    }

    default: {
      const conn = connections.get(fromAgentId);
      conn?.ws.send(
        JSON.stringify({ type: "error", message: `Unknown type: ${type}` })
      );
    }
  }
}

/** Deliver any pending stored messages to a freshly connected agent */
async function deliverPending(
  agentId: string,
  licenseKey: string,
  ws: WebSocket
) {
  // Find agent's project for project-targeted messages
  const [agent] = await db
    .select()
    .from(agents)
    .where(eq(agents.agentId, agentId))
    .limit(1);

  if (!agent) return;

  const pending = await db
    .select()
    .from(relayMessages)
    .where(
      and(
        eq(relayMessages.licenseKey, licenseKey),
        eq(relayMessages.delivered, false),
        or(
          eq(relayMessages.toAgentId, agentId),
          eq(relayMessages.toProject, agent.project)
        )
      )
    )
    .limit(100);

  for (const msg of pending) {
    ws.send(
      JSON.stringify({
        type: "deliver",
        from_agent_id: msg.fromAgentId,
        payload: msg.payload,
      })
    );

    // Mark as delivered
    await db
      .update(relayMessages)
      .set({ delivered: true })
      .where(eq(relayMessages.id, msg.id));
  }

  if (pending.length > 0) {
    console.log(
      `[relay] Delivered ${pending.length} pending messages to ${agentId}`
    );
  }
}
