import type { Context, Next } from "hono";

interface RateEntry {
  count: number;
  resetAt: number;
}

const store = new Map<string, RateEntry>();

/** In-memory per-IP rate limiter */
export function rateLimit(maxRequests: number, windowMs = 60_000) {
  return async (c: Context, next: Next) => {
    const ip =
      c.req.header("x-forwarded-for")?.split(",")[0]?.trim() ||
      c.req.header("x-real-ip") ||
      "unknown";

    const now = Date.now();
    let entry = store.get(ip);

    if (!entry || now > entry.resetAt) {
      entry = { count: 0, resetAt: now + windowMs };
      store.set(ip, entry);
    }

    entry.count++;

    if (entry.count > maxRequests) {
      return c.json({ error: "Too many requests" }, 429);
    }

    return next();
  };
}
