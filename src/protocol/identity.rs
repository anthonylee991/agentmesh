use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique agent identity within the AgentMesh network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub agent_id: String,
    pub name: String,
    pub project: String,
    pub project_path: Option<String>,
    pub platform: AgentPlatform,
    pub capabilities: Vec<AgentCapability>,
    pub status: AgentStatus,
    pub registered_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentPlatform {
    ClaudeCode,
    ChatGpt,
    Cursor,
    Copilot,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentCapability {
    CodeReview,
    ProjectStatus,
    CodeEdit,
    Testing,
    DomainExpert,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Online,
    Busy,
    Offline,
}

impl AgentIdentity {
    pub fn new(
        name: String,
        project: String,
        project_path: Option<String>,
        platform: AgentPlatform,
        capabilities: Vec<AgentCapability>,
    ) -> Self {
        let now = Utc::now();
        Self {
            agent_id: uuid::Uuid::new_v4().to_string(),
            name,
            project,
            project_path,
            platform,
            capabilities,
            status: AgentStatus::Online,
            registered_at: now,
            last_seen: now,
            user_id: None,
        }
    }
}
