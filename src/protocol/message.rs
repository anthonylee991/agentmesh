use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Default TTL when none is specified (1 hour).
const DEFAULT_TTL: u32 = 3600;

/// A message in the AgentMesh protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshMessage {
    pub id: String,
    pub from: String,
    /// Recipient: agent_id, project name, or "*" for broadcast.
    pub to: String,
    pub msg_type: MessageType,
    pub content: MessageContent,
    pub project: Option<String>,
    /// Links a response to its original question.
    pub correlation_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    /// Time-to-live in seconds.
    pub ttl: u32,
    /// Whether this was answered by a proxy agent.
    pub proxy_response: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Ask,
    Response,
    Broadcast,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContent {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
}

impl MeshMessage {
    pub fn ask(from: &str, to: &str, question: &str) -> Self {
        Self::ask_with_ttl(from, to, question, DEFAULT_TTL)
    }

    pub fn ask_with_ttl(from: &str, to: &str, question: &str, ttl: u32) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from: from.to_string(),
            to: to.to_string(),
            msg_type: MessageType::Ask,
            content: MessageContent {
                text: question.to_string(),
                data: None,
                attachments: vec![],
            },
            project: None,
            correlation_id: None,
            timestamp: Utc::now(),
            ttl,
            proxy_response: false,
        }
    }

    pub fn response(from: &str, original: &MeshMessage, answer: &str) -> Self {
        Self::response_with_ttl(from, original, answer, DEFAULT_TTL)
    }

    pub fn response_with_ttl(from: &str, original: &MeshMessage, answer: &str, ttl: u32) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from: from.to_string(),
            to: original.from.clone(),
            msg_type: MessageType::Response,
            content: MessageContent {
                text: answer.to_string(),
                data: None,
                attachments: vec![],
            },
            project: original.project.clone(),
            correlation_id: Some(original.id.clone()),
            timestamp: Utc::now(),
            ttl,
            proxy_response: false,
        }
    }

    pub fn system(to: &str, text: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from: "system".to_string(),
            to: to.to_string(),
            msg_type: MessageType::System,
            content: MessageContent {
                text: text.to_string(),
                data: None,
                attachments: vec![],
            },
            project: None,
            correlation_id: None,
            timestamp: Utc::now(),
            ttl: DEFAULT_TTL,
            proxy_response: false,
        }
    }

    pub fn is_expired(&self) -> bool {
        let elapsed = Utc::now()
            .signed_duration_since(self.timestamp)
            .num_seconds();
        elapsed > self.ttl as i64
    }
}
