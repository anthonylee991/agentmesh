use serde::{Deserialize, Serialize};

use crate::protocol::identity::{AgentCapability, AgentIdentity, AgentPlatform};
use crate::protocol::message::MeshMessage;

/// All operations that flow over the wire between agents and the broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "payload")]
#[serde(rename_all = "snake_case")]
pub enum MeshOperation {
    // Agent lifecycle
    Register(RegisterPayload),
    Registered(RegisteredPayload),
    Deregister,
    Heartbeat,

    // Discovery
    Discover(DiscoverPayload),
    DiscoverResult(DiscoverResultPayload),

    // Messaging
    Send(MeshMessage),
    Deliver(MeshMessage),
    Ack { message_id: String },

    // Status
    Status,
    StatusResult(StatusResultPayload),

    // Error
    Error(ErrorPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterPayload {
    pub name: String,
    pub project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
    #[serde(default)]
    pub platform: Option<AgentPlatform>,
    #[serde(default)]
    pub capabilities: Vec<AgentCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredPayload {
    pub agent_id: String,
    pub broker_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<AgentCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<AgentPlatform>,
    #[serde(default = "default_true")]
    pub online_only: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverResultPayload {
    pub agents: Vec<AgentIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResultPayload {
    pub broker_version: String,
    pub uptime_secs: u64,
    pub connected_agents: usize,
    pub total_messages_routed: u64,
    pub pending_messages: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: u32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}
