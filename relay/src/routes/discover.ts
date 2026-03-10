import { Hono } from "hono";
import type { Env } from "../types.js";
import { db } from "../db/index.js";
import { agents } from "../db/schema.js";
import { eq, and } from "drizzle-orm";
import { licenseAuth } from "../middleware/auth.js";

const app = new Hono<Env>();

/** Discover agents on the relay network.
 *  Can filter by project, platform, online_only.
 *  Only returns agents belonging to the same license. */
app.get("/mesh/discover", licenseAuth, async (c) => {
  const licenseKey = c.get("licenseKey") as string;
  const project = c.req.query("project");
  const platform = c.req.query("platform");
  const onlineOnly = c.req.query("online_only") === "true";

  const conditions = [eq(agents.licenseKey, licenseKey)];

  if (project) {
    conditions.push(eq(agents.project, project));
  }
  if (platform) {
    conditions.push(eq(agents.platform, platform));
  }
  if (onlineOnly) {
    conditions.push(eq(agents.online, true));
  }

  const rows = await db
    .select()
    .from(agents)
    .where(and(...conditions));

  const result = rows.map((row) => ({
    agent_id: row.agentId,
    name: row.name,
    project: row.project,
    platform: row.platform,
    capabilities: row.capabilities ? JSON.parse(row.capabilities) : [],
    online: row.online,
    last_seen: row.lastSeen,
  }));

  return c.json({ agents: result });
});

export default app;
