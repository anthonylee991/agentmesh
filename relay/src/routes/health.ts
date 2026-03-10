import { Hono } from "hono";

const app = new Hono();

app.get("/health", (c) => {
  return c.json({
    status: "ok",
    service: "agentmesh-relay",
    version: "0.1.0",
  });
});

export default app;
