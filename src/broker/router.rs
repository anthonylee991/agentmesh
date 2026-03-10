use std::sync::Arc;

use tokio::sync::Mutex;

use crate::protocol::message::{MeshMessage, MessageType};
use crate::protocol::operations::MeshOperation;

use super::registry::AgentRegistry;

/// Routes messages between agents. Fully async — delivers and returns immediately.
pub struct MessageRouter {
    registry: Arc<Mutex<AgentRegistry>>,
}

impl MessageRouter {
    pub fn new(registry: Arc<Mutex<AgentRegistry>>) -> Self {
        Self { registry }
    }

    /// Route a message to its recipient(s). Returns an ack or error, never blocks.
    pub async fn route(&self, message: MeshMessage) -> anyhow::Result<Option<MeshMessage>> {
        match message.msg_type {
            MessageType::Ask => self.route_ask(message).await,
            MessageType::Response => {
                self.route_response(message).await?;
                Ok(None)
            }
            MessageType::Broadcast => {
                self.route_broadcast(message).await?;
                Ok(None)
            }
            MessageType::System => {
                self.route_direct(&message.to, &message).await?;
                Ok(None)
            }
        }
    }

    /// Route an Ask: deliver to the target agent and return immediately.
    /// The response will come back asynchronously via route_response.
    async fn route_ask(&self, message: MeshMessage) -> anyhow::Result<Option<MeshMessage>> {
        let registry = self.registry.lock().await;

        // Find target: by agent_id first, then by agent name, then by project name
        let target_ids: Vec<String> = if registry.get_identity(&message.to).is_some() {
            vec![message.to.clone()]
        } else {
            // Try agent name first, then project name
            let mut candidates = registry.agents_by_name(&message.to);
            if candidates.is_empty() {
                candidates = registry.agents_for_project(&message.to);
            }
            // Filter out the sender so we don't deliver to ourselves
            candidates
                .into_iter()
                .filter(|id| *id != message.from)
                .collect()
        };

        if target_ids.is_empty() {
            drop(registry);
            let error_msg = MeshMessage::system(
                &message.from,
                &format!(
                    "No agent found for '{}'. Use mesh_discover to see available agents and projects.",
                    message.to
                ),
            );
            return Ok(Some(error_msg));
        }

        // Deliver to the first available agent
        let target_id = &target_ids[0];
        let sender = registry.get_sender(target_id);
        let target_name = registry
            .get_identity(target_id)
            .map(|id| id.name.clone())
            .unwrap_or_else(|| target_id.to_string());
        drop(registry);

        let Some(sender) = sender else {
            let error_msg = MeshMessage::system(
                &message.from,
                &format!("Agent '{}' is registered but not reachable.", message.to),
            );
            return Ok(Some(error_msg));
        };

        // Deliver the message — the target agent will see it as a dynamic inbox tool
        let deliver_op = MeshOperation::Deliver(message.clone());
        let deliver_json = serde_json::to_string(&deliver_op)?;

        if sender.send(deliver_json).is_err() {
            let error_msg = MeshMessage::system(
                &message.from,
                &format!("Agent '{}' disconnected.", message.to),
            );
            return Ok(Some(error_msg));
        }

        // Return an ack — the real response will arrive async via the sender's inbox
        let ack = MeshMessage::system(
            &message.from,
            &format!(
                "Message delivered to '{}'. Their response will appear as a mesh_inbox tool when ready.",
                target_name
            ),
        );
        Ok(Some(ack))
    }

    /// Route a Response message directly to the original sender.
    async fn route_response(&self, message: MeshMessage) -> anyhow::Result<()> {
        self.route_direct(&message.to, &message).await
    }

    /// Broadcast a message to all connected agents (except sender).
    async fn route_broadcast(&self, message: MeshMessage) -> anyhow::Result<()> {
        let registry = self.registry.lock().await;
        let all_agents = registry.discover(None, None, None, true);

        for agent in &all_agents {
            if agent.agent_id == message.from {
                continue;
            }
            if let Some(sender) = registry.get_sender(&agent.agent_id) {
                let deliver_op = MeshOperation::Deliver(message.clone());
                if let Ok(json) = serde_json::to_string(&deliver_op) {
                    let _ = sender.send(json);
                }
            }
        }

        Ok(())
    }

    /// Send a message directly to a specific agent.
    async fn route_direct(&self, agent_id: &str, message: &MeshMessage) -> anyhow::Result<()> {
        let registry = self.registry.lock().await;
        if let Some(sender) = registry.get_sender(agent_id) {
            let deliver_op = MeshOperation::Deliver(message.clone());
            let json = serde_json::to_string(&deliver_op)?;
            let _ = sender.send(json);
        }
        Ok(())
    }
}
