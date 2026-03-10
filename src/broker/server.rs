use std::sync::Arc;

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;

use crate::config::AppConfig;
use crate::protocol::operations::{
    DiscoverResultPayload, ErrorPayload, MeshOperation, RegisteredPayload, StatusResultPayload,
};

use super::BrokerState;

const BROKER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Start the broker server on the configured port.
pub async fn start_broker(config: AppConfig) -> anyhow::Result<()> {
    let addr = format!("{}:{}", config.broker.host, config.broker.port);
    let state = Arc::new(BrokerState::new(config));

    // Spawn a background task to prune stale agents every 60 seconds
    let prune_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let mut registry = prune_state.registry.lock().await;
            registry.prune_stale(120);
        }
    });

    // Start relay client if Pro tier is configured
    crate::transport::relay_client::maybe_start_relay(Arc::clone(&state));

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/sse", get(crate::transport::sse::sse_handler))
        .route("/message", post(crate::transport::sse::message_handler))
        .route("/health", get(health_handler))
        .route("/api/status", get(status_handler))
        .route("/api/agents", get(agents_handler))
        .with_state(Arc::clone(&state))
        .layer(CorsLayer::very_permissive());

    tracing::info!(addr = %addr, "AgentMesh broker starting");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// --- Handlers ---

async fn health_handler() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "service": "agentmesh-broker",
        "version": BROKER_VERSION,
    }))
}

async fn status_handler(State(state): State<Arc<BrokerState>>) -> impl IntoResponse {
    let registry = state.registry.lock().await;
    let result = StatusResultPayload {
        broker_version: BROKER_VERSION.to_string(),
        uptime_secs: state.uptime_secs(),
        connected_agents: registry.count(),
        total_messages_routed: state.total_messages(),
        pending_messages: 0,
    };
    axum::Json(serde_json::to_value(result).unwrap())
}

async fn agents_handler(State(state): State<Arc<BrokerState>>) -> impl IntoResponse {
    let registry = state.registry.lock().await;
    let agents = registry.discover(None, None, None, false);
    axum::Json(serde_json::to_value(agents).unwrap())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<BrokerState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_agent_connection(socket, state))
}

/// Handle a single agent's WebSocket connection lifecycle.
async fn handle_agent_connection(stream: WebSocket, state: Arc<BrokerState>) {
    let (mut ws_sender, mut ws_receiver) = stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Spawn a task to forward messages from the channel to the WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sender.send(WsMessage::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let mut agent_id: Option<String> = None;

    // Process incoming messages from the agent
    while let Some(Ok(msg)) = ws_receiver.next().await {
        match msg {
            WsMessage::Text(text) => {
                let text_ref: &str = &text;
                let op = match serde_json::from_str::<MeshOperation>(text_ref) {
                    Ok(op) => op,
                    Err(e) => {
                        let error_op = MeshOperation::Error(ErrorPayload {
                            code: 400,
                            message: format!("Invalid operation: {}", e),
                            correlation_id: None,
                        });
                        if let Ok(json) = serde_json::to_string(&error_op) {
                            let _ = tx.send(json);
                        }
                        continue;
                    }
                };

                match op {
                    MeshOperation::Register(payload) => {
                        let mut registry = state.registry.lock().await;
                        let id = registry.register(payload, tx.clone());
                        agent_id = Some(id.clone());

                        let response = MeshOperation::Registered(RegisteredPayload {
                            agent_id: id,
                            broker_version: BROKER_VERSION.to_string(),
                        });
                        if let Ok(json) = serde_json::to_string(&response) {
                            let _ = tx.send(json);
                        }
                    }

                    MeshOperation::Deregister => {
                        if let Some(id) = &agent_id {
                            let mut registry = state.registry.lock().await;
                            registry.deregister(id);
                        }
                        break;
                    }

                    MeshOperation::Heartbeat => {
                        if let Some(id) = &agent_id {
                            let mut registry = state.registry.lock().await;
                            registry.heartbeat(id);
                        }
                    }

                    MeshOperation::Discover(payload) => {
                        let registry = state.registry.lock().await;
                        let agents = registry.discover(
                            payload.project.as_deref(),
                            payload.capability.as_ref(),
                            payload.platform.as_ref(),
                            payload.online_only,
                        );
                        drop(registry);

                        let response = MeshOperation::DiscoverResult(DiscoverResultPayload {
                            agents,
                        });
                        if let Ok(json) = serde_json::to_string(&response) {
                            let _ = tx.send(json);
                        }
                    }

                    MeshOperation::Send(message) => {
                        state.increment_messages();
                        let router = Arc::clone(&state.router);

                        // Route in a separate task so we don't block the WS read loop
                        let tx_clone = tx.clone();
                        tokio::spawn(async move {
                            match router.route(message).await {
                                Ok(Some(response)) => {
                                    let deliver =
                                        MeshOperation::Deliver(response);
                                    if let Ok(json) = serde_json::to_string(&deliver) {
                                        let _ = tx_clone.send(json);
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    let error_op = MeshOperation::Error(ErrorPayload {
                                        code: 500,
                                        message: format!("Routing error: {}", e),
                                        correlation_id: None,
                                    });
                                    if let Ok(json) = serde_json::to_string(&error_op) {
                                        let _ = tx_clone.send(json);
                                    }
                                }
                            }
                        });
                    }

                    MeshOperation::Status => {
                        let registry = state.registry.lock().await;
                        let response = MeshOperation::StatusResult(StatusResultPayload {
                            broker_version: BROKER_VERSION.to_string(),
                            uptime_secs: state.uptime_secs(),
                            connected_agents: registry.count(),
                            total_messages_routed: state.total_messages(),
                            pending_messages: 0,
                        });
                        drop(registry);

                        if let Ok(json) = serde_json::to_string(&response) {
                            let _ = tx.send(json);
                        }
                    }

                    // Ack, Deliver, etc. -- handled by router or ignored
                    _ => {}
                }
            }

            // Axum handles pong automatically for pings
            WsMessage::Ping(_) => {}
            WsMessage::Close(_) => break,
            _ => {}
        }
    }

    // Cleanup on disconnect
    if let Some(id) = agent_id {
        let mut registry = state.registry.lock().await;
        registry.deregister(&id);
    }

    send_task.abort();
    tracing::info!("Agent connection closed");
}
