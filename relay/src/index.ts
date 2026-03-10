import { Hono } from "hono";
import { cors } from "hono/cors";
import { serve } from "@hono/node-server";
import healthRoutes from "./routes/health.js";
import registerRoutes from "./routes/register.js";
import discoverRoutes from "./routes/discover.js";
import { setupWebSocketRelay } from "./routes/relay.js";

const app = new Hono();

// Global middleware
app.use("*", cors());

// Routes
app.route("/", healthRoutes);
app.route("/", registerRoutes);
app.route("/", discoverRoutes);

const port = parseInt(process.env.PORT || "3456");

console.log(`[agentmesh-relay] Starting on port ${port}`);

const server = serve({ fetch: app.fetch, port }, (info) => {
  console.log(`[agentmesh-relay] Listening on http://localhost:${info.port}`);
});

// Attach WebSocket relay to the HTTP server
setupWebSocketRelay(server as any);
