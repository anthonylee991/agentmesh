import { Hono } from "hono";
import type { Env } from "../types.js";
import { db } from "../db/index.js";
import { agents } from "../db/schema.js";
import { eq } from "drizzle-orm";
import { licenseAuth } from "../middleware/auth.js";
import { rateLimit } from "../middleware/rate-limit.js";

const app = new Hono<Env>();

interface RegisterBody {
  agent_id: string;
  name: string;
  project: string;
  platform?: string;
  capabilities?: string[];
}

/** Register an agent on the relay network */
app.post("/mesh/register", rateLimit(10), licenseAuth, async (c) => {
  const licenseKey = c.get("licenseKey") as string;
  const body = await c.req.json<RegisterBody>();

  if (!body.agent_id || !body.name || !body.project) {
    return c.json({ error: "agent_id, name, and project are required" }, 400);
  }

  await db
    .insert(agents)
    .values({
      agentId: body.agent_id,
      licenseKey,
      name: body.name,
      project: body.project,
      platform: body.platform || null,
      capabilities: body.capabilities
        ? JSON.stringify(body.capabilities)
        : null,
      online: true,
      lastSeen: new Date(),
    })
    .onConflictDoUpdate({
      target: agents.agentId,
      set: {
        name: body.name,
        project: body.project,
        platform: body.platform || null,
        capabilities: body.capabilities
          ? JSON.stringify(body.capabilities)
          : null,
        online: true,
        lastSeen: new Date(),
      },
    });

  return c.json({ status: "ok", agent_id: body.agent_id });
});

/** Deregister an agent */
app.post("/mesh/deregister", licenseAuth, async (c) => {
  const { agent_id } = await c.req.json<{ agent_id: string }>();
  const licenseKey = c.get("licenseKey") as string;

  await db
    .update(agents)
    .set({ online: false })
    .where(eq(agents.agentId, agent_id));

  return c.json({ status: "ok" });
});

export default app;
