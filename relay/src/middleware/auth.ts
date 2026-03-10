import type { Context, Next } from "hono";
import type { Env } from "../types.js";
import { db } from "../db/index.js";
import { users } from "../db/schema.js";
import { eq } from "drizzle-orm";

/** Validate license key from X-License-Key header */
export async function licenseAuth(c: Context<Env>, next: Next) {
  const licenseKey = c.req.header("X-License-Key");
  if (!licenseKey) {
    return c.json({ error: "Missing X-License-Key header" }, 401);
  }

  // Check local DB first
  const [user] = await db
    .select()
    .from(users)
    .where(eq(users.licenseKey, licenseKey))
    .limit(1);

  if (!user) {
    // Validate against Supabase if not in local DB
    const valid = await validateSupabaseLicense(licenseKey);
    if (!valid) {
      return c.json({ error: "Invalid license key" }, 403);
    }

    // Insert new user
    await db.insert(users).values({ licenseKey }).onConflictDoNothing();
  }

  // Update last_seen
  await db
    .update(users)
    .set({ lastSeen: new Date() })
    .where(eq(users.licenseKey, licenseKey));

  c.set("licenseKey", licenseKey);
  return next();
}

/** Validate license against Supabase purchases table */
async function validateSupabaseLicense(key: string): Promise<boolean> {
  const supabaseUrl = process.env.SUPABASE_URL;
  const supabaseKey = process.env.SUPABASE_SERVICE_KEY;

  if (!supabaseUrl || !supabaseKey) {
    console.warn(
      "SUPABASE_URL/SUPABASE_SERVICE_KEY not set — skipping license validation"
    );
    return false;
  }

  try {
    const res = await fetch(
      `${supabaseUrl}/rest/v1/purchases?token=eq.${key}&product_id=eq.agentmesh_pro&select=id`,
      {
        headers: {
          apikey: supabaseKey,
          Authorization: `Bearer ${supabaseKey}`,
        },
      }
    );

    if (!res.ok) return false;
    const rows = (await res.json()) as unknown[];
    return rows.length > 0;
  } catch {
    return false;
  }
}
