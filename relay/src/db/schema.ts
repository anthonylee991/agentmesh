import {
  pgTable,
  varchar,
  text,
  timestamp,
  bigserial,
  uuid,
  index,
  boolean,
} from "drizzle-orm/pg-core";

/** Licensed users — validated against Supabase purchases */
export const users = pgTable("users", {
  licenseKey: uuid("license_key").primaryKey(),
  email: varchar("email", { length: 255 }),
  createdAt: timestamp("created_at", { withTimezone: true }).defaultNow(),
  lastSeen: timestamp("last_seen", { withTimezone: true }).defaultNow(),
});

/** Registered agents across the relay network */
export const agents = pgTable(
  "agents",
  {
    agentId: varchar("agent_id", { length: 255 }).primaryKey(),
    licenseKey: uuid("license_key")
      .notNull()
      .references(() => users.licenseKey, { onDelete: "cascade" }),
    name: varchar("name", { length: 255 }).notNull(),
    project: varchar("project", { length: 255 }).notNull(),
    platform: varchar("platform", { length: 100 }),
    capabilities: text("capabilities"), // JSON array stored as text
    online: boolean("online").default(false),
    lastSeen: timestamp("last_seen", { withTimezone: true }).defaultNow(),
    createdAt: timestamp("created_at", { withTimezone: true }).defaultNow(),
  },
  (table) => [
    index("idx_agents_license").on(table.licenseKey),
    index("idx_agents_project").on(table.project),
  ]
);

/** Store-and-forward relay messages (encrypted, zero-knowledge) */
export const relayMessages = pgTable(
  "relay_messages",
  {
    id: bigserial("id", { mode: "number" }).primaryKey(),
    fromAgentId: varchar("from_agent_id", { length: 255 }).notNull(),
    toAgentId: varchar("to_agent_id", { length: 255 }),
    toProject: varchar("to_project", { length: 255 }),
    licenseKey: uuid("license_key")
      .notNull()
      .references(() => users.licenseKey, { onDelete: "cascade" }),
    payload: text("payload").notNull(), // Encrypted JSON blob — relay never sees plaintext
    delivered: boolean("delivered").default(false),
    createdAt: timestamp("created_at", { withTimezone: true }).defaultNow(),
  },
  (table) => [
    index("idx_relay_messages_to_agent").on(table.toAgentId, table.delivered),
    index("idx_relay_messages_to_project").on(
      table.toProject,
      table.delivered
    ),
    index("idx_relay_messages_license").on(table.licenseKey),
  ]
);
