use std::sync::Arc;
use std::time::Duration;

use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::sync::Mutex;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use crate::broker::BrokerState;
use crate::protocol::message::MeshMessage;

/// Relay client that maintains a persistent WebSocket connection
/// to the AgentMesh cloud relay for cross-network routing.
pub struct RelayClient {
    relay_url: String,
    license_key: String,
    agent_id: String,
    state: Arc<BrokerState>,
}

impl RelayClient {
    pub fn new(
        relay_url: String,
        license_key: String,
        agent_id: String,
        state: Arc<BrokerState>,
    ) -> Self {
        Self {
            relay_url,
            license_key,
            agent_id,
            state,
        }
    }

    /// Start the relay client with auto-reconnect and exponential backoff.
    pub async fn run(self: Arc<Self>) {
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(60);

        loop {
            match self.connect_and_run().await {
                Ok(()) => {
                    tracing::info!("Relay connection closed cleanly");
                    backoff = Duration::from_secs(1); // Reset on clean close
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        backoff_secs = backoff.as_secs(),
                        "Relay connection failed, reconnecting"
                    );
                }
            }

            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
        }
    }

    /// Single connection lifecycle.
    async fn connect_and_run(&self) -> anyhow::Result<()> {
        let url = format!(
            "{}/mesh/ws?agent_id={}&license_key={}",
            self.relay_url, self.agent_id, self.license_key
        );

        tracing::info!(url = %self.relay_url, "Connecting to relay");

        let (ws_stream, _) = connect_async(&url).await?;
        let (write, mut read) = ws_stream.split();
        let write = Arc::new(Mutex::new(write));

        // Spawn heartbeat task — ping every 30s
        let write_hb = Arc::clone(&write);
        let heartbeat = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let mut w = write_hb.lock().await;
                if w.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
        });

        // Process incoming messages from relay
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    self.handle_relay_message(&text, &write).await;
                }
                Ok(Message::Ping(_)) => {
                    let mut w = write.lock().await;
                    let _ = w.send(Message::Pong(vec![].into())).await;
                }
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    tracing::warn!(error = %e, "Relay WebSocket error");
                    break;
                }
                _ => {}
            }
        }

        heartbeat.abort();
        Ok(())
    }

    /// Handle a message received from the relay.
    async fn handle_relay_message(
        &self,
        text: &str,
        _write: &Arc<Mutex<impl futures::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin>>,
    ) {
        let msg: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "Invalid relay message");
                return;
            }
        };

        let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match msg_type {
            "connected" => {
                tracing::info!("Connected to relay, acknowledged");
            }

            "deliver" => {
                // A message relayed from a remote agent — decrypt and route locally
                let payload = match msg.get("payload").and_then(|v| v.as_str()) {
                    Some(p) => p,
                    None => return,
                };
                let from = msg
                    .get("from_agent_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("relay:unknown");

                // The payload is an encrypted MeshMessage JSON string.
                // For now, treat as plaintext (encryption is Phase 5b).
                match serde_json::from_str::<MeshMessage>(payload) {
                    Ok(message) => {
                        tracing::info!(
                            from = %from,
                            to = %message.to,
                            "Received relayed message"
                        );
                        // Route the message through the local broker
                        let router = Arc::clone(&self.state.router);
                        tokio::spawn(async move {
                            if let Err(e) = router.route(message).await {
                                tracing::warn!(error = %e, "Failed to route relayed message");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to parse relayed message payload");
                    }
                }
            }

            "queued" => {
                tracing::debug!("Message queued on relay for later delivery");
            }

            "error" => {
                let error_msg = msg
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                tracing::warn!(error = %error_msg, "Relay error");
            }

            _ => {
                tracing::debug!(msg_type = %msg_type, "Unknown relay message type");
            }
        }
    }

    /// Send a message through the relay to a remote agent.
    pub async fn relay_message(
        &self,
        message: &MeshMessage,
        to_agent_id: Option<&str>,
        to_project: Option<&str>,
    ) -> anyhow::Result<()> {
        // For now, plaintext. Encryption will be added.
        let payload = serde_json::to_string(message)?;

        let relay_msg = serde_json::json!({
            "type": "relay",
            "to_agent_id": to_agent_id,
            "to_project": to_project,
            "payload": payload,
        });

        tracing::info!(
            to_agent = ?to_agent_id,
            to_project = ?to_project,
            "Relaying message to cloud"
        );

        // This would need access to the write half — for now, log intent.
        // Full implementation requires sharing the write handle.
        tracing::debug!(msg = %relay_msg, "Relay message prepared");

        Ok(())
    }
}

/// Spawn the relay client as a background task if Pro tier is configured.
pub fn maybe_start_relay(state: Arc<BrokerState>) {
    let config = &state.config;

    let license_key = match &config.pro.license_key {
        Some(k) if !k.is_empty() => k.clone(),
        _ => {
            tracing::debug!("Pro tier not configured, skipping relay connection");
            return;
        }
    };
    let relay_url = match &config.pro.relay_url {
        Some(u) if !u.is_empty() => u.clone(),
        _ => {
            tracing::debug!("Pro relay URL not set, skipping relay connection");
            return;
        }
    };

    // Use a broker-level agent ID for the relay connection
    let agent_id = format!("broker:{}", uuid::Uuid::new_v4());

    let client = Arc::new(RelayClient::new(
        relay_url,
        license_key,
        agent_id,
        Arc::clone(&state),
    ));

    tokio::spawn(async move {
        client.run().await;
    });

    tracing::info!("Relay client started (Pro tier)");
}
