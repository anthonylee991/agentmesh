pub mod proxy;
pub mod registry;
pub mod router;
pub mod server;

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::config::AppConfig;

use self::registry::AgentRegistry;
use self::router::MessageRouter;

/// Central broker state, shared across all transport handlers.
pub struct BrokerState {
    pub registry: Arc<Mutex<AgentRegistry>>,
    pub router: Arc<MessageRouter>,
    pub config: AppConfig,
    pub start_time: std::time::Instant,
    pub messages_routed: Arc<AtomicU64>,
    /// SSE session senders: session_id -> channel to push events to SSE stream
    pub sse_sessions: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>>,
    /// Maps SSE session_id -> agent_id for cleanup on disconnect
    pub sse_agent_map: Arc<Mutex<HashMap<String, String>>>,
}

impl BrokerState {
    pub fn new(config: AppConfig) -> Self {
        let registry = Arc::new(Mutex::new(AgentRegistry::new()));
        let router = Arc::new(MessageRouter::new(Arc::clone(&registry)));

        Self {
            registry,
            router,
            config,
            start_time: std::time::Instant::now(),
            messages_routed: Arc::new(AtomicU64::new(0)),
            sse_sessions: Arc::new(Mutex::new(HashMap::new())),
            sse_agent_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    pub fn total_messages(&self) -> u64 {
        self.messages_routed
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn increment_messages(&self) {
        self.messages_routed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}
