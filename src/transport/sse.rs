use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use futures::stream::Stream;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;

use crate::broker::BrokerState;
use crate::protocol::operations::{
    DiscoverResultPayload, ErrorPayload, MeshOperation, RegisteredPayload, StatusResultPayload,
};

const BROKER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize)]
pub struct SessionQuery {
    pub session_id: String,
}

/// SSE endpoint: browser connects here to receive events.
/// Returns the message endpoint URL as the first event, then streams messages.
pub async fn sse_handler(
    State(state): State<Arc<BrokerState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::unbounded_channel::<String>();

    // Store the session sender
    {
        let mut sessions = state.sse_sessions.lock().await;
        sessions.insert(session_id.clone(), tx);
    }

    tracing::info!(session_id = %session_id, "SSE session opened");

    let session_id_clone = session_id.clone();
    let state_clone = Arc::clone(&state);

    // Build the event stream
    let stream = async_stream::stream! {
        // First event: send the message endpoint URL
        let endpoint = format!("/message?session_id={}", session_id_clone);
        yield Ok(Event::default().event("endpoint").data(endpoint));

        // Stream messages from the channel
        let mut rx_stream = UnboundedReceiverStream::new(rx);
        while let Some(msg) = rx_stream.next().await {
            yield Ok(Event::default().event("message").data(msg));
        }

        // Cleanup on disconnect
        let mut sessions = state_clone.sse_sessions.lock().await;
        sessions.remove(&session_id_clone);

        // Deregister the agent if one was registered on this session
        let mut registry = state_clone.registry.lock().await;
        let mut sse_agents = state_clone.sse_agent_map.lock().await;
        if let Some(agent_id) = sse_agents.remove(&session_id_clone) {
            registry.deregister(&agent_id);
        }

        tracing::info!(session_id = %session_id_clone, "SSE session closed");
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

/// Message endpoint: browser POSTs operations here.
pub async fn message_handler(
    State(state): State<Arc<BrokerState>>,
    Query(query): Query<SessionQuery>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let session_id = &query.session_id;

    // Check session exists
    let sessions = state.sse_sessions.lock().await;
    let tx = match sessions.get(session_id) {
        Some(tx) => tx.clone(),
        None => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Session not found"})),
            );
        }
    };
    drop(sessions);

    // Parse the operation
    let op: MeshOperation = match serde_json::from_value(body) {
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
            return (
                axum::http::StatusCode::ACCEPTED,
                Json(serde_json::json!({"status": "error_sent"})),
            );
        }
    };

    // Process the operation (same logic as WebSocket handler)
    match op {
        MeshOperation::Register(payload) => {
            let mut registry = state.registry.lock().await;
            let agent_id = registry.register(payload, tx.clone());

            // Map this SSE session to the agent ID for cleanup
            let mut sse_agents = state.sse_agent_map.lock().await;
            sse_agents.insert(session_id.to_string(), agent_id.clone());

            let response = MeshOperation::Registered(RegisteredPayload {
                agent_id,
                broker_version: BROKER_VERSION.to_string(),
            });
            if let Ok(json) = serde_json::to_string(&response) {
                let _ = tx.send(json);
            }
        }

        MeshOperation::Deregister => {
            let mut sse_agents = state.sse_agent_map.lock().await;
            if let Some(agent_id) = sse_agents.remove(session_id) {
                let mut registry = state.registry.lock().await;
                registry.deregister(&agent_id);
            }
        }

        MeshOperation::Heartbeat => {
            let sse_agents = state.sse_agent_map.lock().await;
            if let Some(agent_id) = sse_agents.get(session_id) {
                let mut registry = state.registry.lock().await;
                registry.heartbeat(agent_id);
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

            let response = MeshOperation::DiscoverResult(DiscoverResultPayload { agents });
            if let Ok(json) = serde_json::to_string(&response) {
                let _ = tx.send(json);
            }
        }

        MeshOperation::Send(message) => {
            state.increment_messages();
            let router = Arc::clone(&state.router);
            let tx_clone = tx.clone();

            tokio::spawn(async move {
                match router.route(message).await {
                    Ok(Some(response)) => {
                        let deliver = MeshOperation::Deliver(response);
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

        _ => {}
    }

    (
        axum::http::StatusCode::ACCEPTED,
        Json(serde_json::json!({"status": "accepted"})),
    )
}
