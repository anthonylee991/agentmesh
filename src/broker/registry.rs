use chrono::Utc;
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::protocol::identity::{AgentCapability, AgentIdentity, AgentPlatform, AgentStatus};
use crate::protocol::operations::RegisterPayload;

/// An agent entry in the registry: identity + channel to push messages.
pub struct AgentEntry {
    pub identity: AgentIdentity,
    pub tx: mpsc::UnboundedSender<String>,
}

/// In-memory registry of connected agents.
pub struct AgentRegistry {
    agents: HashMap<String, AgentEntry>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    /// Register a new agent. Returns the assigned agent_id.
    pub fn register(
        &mut self,
        payload: RegisterPayload,
        tx: mpsc::UnboundedSender<String>,
    ) -> String {
        let identity = AgentIdentity::new(
            payload.name,
            payload.project,
            payload.project_path,
            payload
                .platform
                .unwrap_or(AgentPlatform::Custom("unknown".to_string())),
            payload.capabilities,
        );

        let agent_id = identity.agent_id.clone();
        self.agents
            .insert(agent_id.clone(), AgentEntry { identity, tx });

        tracing::info!(agent_id = %agent_id, "Agent registered");
        agent_id
    }

    /// Remove an agent from the registry.
    pub fn deregister(&mut self, agent_id: &str) -> Option<AgentEntry> {
        let entry = self.agents.remove(agent_id);
        if entry.is_some() {
            tracing::info!(agent_id = %agent_id, "Agent deregistered");
        }
        entry
    }

    /// Update last_seen timestamp for heartbeat.
    pub fn heartbeat(&mut self, agent_id: &str) {
        if let Some(entry) = self.agents.get_mut(agent_id) {
            entry.identity.last_seen = Utc::now();
        }
    }

    /// Find agents matching discovery criteria.
    pub fn discover(
        &self,
        project: Option<&str>,
        capability: Option<&AgentCapability>,
        platform: Option<&AgentPlatform>,
        online_only: bool,
    ) -> Vec<AgentIdentity> {
        self.agents
            .values()
            .filter(|entry| {
                if online_only && entry.identity.status == AgentStatus::Offline {
                    return false;
                }
                if let Some(p) = project {
                    if !entry.identity.project.eq_ignore_ascii_case(p) {
                        return false;
                    }
                }
                if let Some(c) = capability {
                    if !entry.identity.capabilities.contains(c) {
                        return false;
                    }
                }
                if let Some(pl) = platform {
                    if entry.identity.platform != *pl {
                        return false;
                    }
                }
                true
            })
            .map(|entry| entry.identity.clone())
            .collect()
    }

    /// Get a cloned sender channel for a specific agent.
    pub fn get_sender(&self, agent_id: &str) -> Option<mpsc::UnboundedSender<String>> {
        self.agents.get(agent_id).map(|e| e.tx.clone())
    }

    /// Find agents by project name, returning their IDs.
    pub fn agents_for_project(&self, project: &str) -> Vec<String> {
        self.agents
            .values()
            .filter(|e| {
                e.identity.project.eq_ignore_ascii_case(project)
                    && e.identity.status != AgentStatus::Offline
            })
            .map(|e| e.identity.agent_id.clone())
            .collect()
    }

    /// Get agent identity by ID.
    pub fn get_identity(&self, agent_id: &str) -> Option<&AgentIdentity> {
        self.agents.get(agent_id).map(|e| &e.identity)
    }

    /// Find agents by name, returning their IDs.
    pub fn agents_by_name(&self, name: &str) -> Vec<String> {
        self.agents
            .values()
            .filter(|e| {
                e.identity.name.eq_ignore_ascii_case(name)
                    && e.identity.status != AgentStatus::Offline
            })
            .map(|e| e.identity.agent_id.clone())
            .collect()
    }

    /// Get project_path for a given project name (from any registered agent).
    pub fn get_project_path(&self, project: &str) -> Option<String> {
        self.agents
            .values()
            .find(|e| e.identity.project.eq_ignore_ascii_case(project))
            .and_then(|e| e.identity.project_path.clone())
    }

    pub fn count(&self) -> usize {
        self.agents.len()
    }

    /// Prune agents that haven't sent a heartbeat in > stale_secs.
    pub fn prune_stale(&mut self, stale_secs: i64) -> Vec<String> {
        let now = Utc::now();
        let stale_ids: Vec<String> = self
            .agents
            .iter()
            .filter(|(_, entry)| {
                now.signed_duration_since(entry.identity.last_seen)
                    .num_seconds()
                    > stale_secs
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in &stale_ids {
            self.agents.remove(id);
            tracing::info!(agent_id = %id, "Pruned stale agent");
        }

        stale_ids
    }
}
