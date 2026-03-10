use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;

use crate::protocol::identity::AgentPlatform;
use crate::protocol::message::{MeshMessage, MessageType};
use crate::protocol::operations::{DiscoverPayload, MeshOperation, RegisterPayload};

use super::tools;

const SERVER_NAME: &str = "agentmesh-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: &str = "2024-11-05";
const DEFAULT_BROKER_URL: &str = "ws://127.0.0.1:7777/ws";

#[derive(Serialize, Deserialize)]
pub struct MCPMessage {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(default)]
    pub method: String,
    pub params: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

/// Write pending inbox state to ~/.agentmesh/inbox_<agent_id>.json for watcher-based notification.
/// Each agent gets its own signal file so multiple agents on the same machine don't interfere.
fn write_inbox_signal(agent_id: &str, messages: &[MeshMessage]) {
    let home = dirs_next::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let signal_dir = home.join(".agentmesh");
    let filename = format!("inbox_{}.json", agent_id);
    let signal_path = signal_dir.join(&filename);

    // Only signal for Ask and Response messages — ignore System acks
    let important: Vec<&MeshMessage> = messages
        .iter()
        .filter(|m| m.msg_type == MessageType::Ask || m.msg_type == MessageType::Response)
        .collect();

    if important.is_empty() {
        let _ = std::fs::remove_file(&signal_path);
        return;
    }

    let _ = std::fs::create_dir_all(&signal_dir);

    let entries: Vec<serde_json::Value> = important
        .iter()
        .map(|m| {
            json!({
                "from": m.from,
                "type": format!("{:?}", m.msg_type),
                "preview": if m.content.text.len() > 200 {
                    format!("{}...", &m.content.text[..200])
                } else {
                    m.content.text.clone()
                },
                "id": m.id,
                "timestamp": m.timestamp,
            })
        })
        .collect();

    let payload = json!({
        "count": entries.len(),
        "messages": entries,
    });

    if let Ok(json_str) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(&signal_path, json_str);
    }
}

/// Holds an active broker connection.
struct BrokerConnection {
    tx: mpsc::UnboundedSender<String>,
}

/// The MCP server that bridges Claude Code to the AgentMesh broker.
/// Starts without a broker connection and connects lazily on first tool call.
pub struct MeshMCPServer {
    broker_url: String,
    connection: Arc<Mutex<Option<BrokerConnection>>>,
    inbox: Arc<Mutex<Vec<MeshMessage>>>,
    agent_id: Arc<Mutex<Option<String>>>,
    response_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<MeshOperation>>>>,
    message_ttl_secs: u32,
    watcher_timeout_secs: u64,
}

impl MeshMCPServer {
    /// Create a new MCP server without connecting to the broker yet.
    pub fn new(broker_url: &str) -> Self {
        let config = crate::config::AppConfig::load().unwrap_or_default();
        Self {
            broker_url: broker_url.to_string(),
            connection: Arc::new(Mutex::new(None)),
            inbox: Arc::new(Mutex::new(Vec::new())),
            agent_id: Arc::new(Mutex::new(None)),
            response_rx: Arc::new(Mutex::new(None)),
            message_ttl_secs: config.broker.message_ttl_secs,
            watcher_timeout_secs: config.broker.watcher_timeout_secs,
        }
    }

    /// Ensure we have an active broker connection. Starts broker if needed.
    async fn ensure_connected(&self) -> Result<()> {
        {
            let conn = self.connection.lock().await;
            if conn.is_some() {
                return Ok(());
            }
        }

        // Try connecting directly first
        if let Ok(()) = self.connect_to_broker().await {
            return Ok(());
        }

        // No broker running — start one embedded
        let config = crate::config::AppConfig::load().unwrap_or_default();
        tokio::spawn(async move {
            let _ = crate::broker::server::start_broker(config).await;
        });

        // Retry with backoff
        for attempt in 1..=20u64 {
            tokio::time::sleep(Duration::from_millis(150 * attempt)).await;
            if self.connect_to_broker().await.is_ok() {
                return Ok(());
            }
        }

        anyhow::bail!(
            "Could not connect to AgentMesh broker after 20 attempts. Is port 7777 available?"
        )
    }

    /// Establish WebSocket connection to broker and spawn read/write tasks.
    async fn connect_to_broker(&self) -> Result<()> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(&self.broker_url).await?;
        let (mut ws_write, mut ws_read) = ws_stream.split();

        let (broker_tx, mut broker_rx) = mpsc::unbounded_channel::<String>();
        let (response_tx, response_rx) = mpsc::unbounded_channel::<MeshOperation>();

        // Spawn task: forward outgoing messages to WS
        tokio::spawn(async move {
            while let Some(msg) = broker_rx.recv().await {
                if ws_write
                    .send(TungsteniteMessage::Text(msg.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Spawn task: read incoming messages from WS
        let inbox_clone = Arc::clone(&self.inbox);
        let agent_id_clone = Arc::clone(&self.agent_id);
        tokio::spawn(async move {
            while let Some(Ok(msg)) = ws_read.next().await {
                if let TungsteniteMessage::Text(text) = msg {
                    let text_ref: &str = &text;
                    if let Ok(op) = serde_json::from_str::<MeshOperation>(text_ref) {
                        match &op {
                            MeshOperation::Registered(payload) => {
                                let mut id = agent_id_clone.lock().await;
                                *id = Some(payload.agent_id.clone());
                                let _ = response_tx.send(op);
                            }
                            MeshOperation::Deliver(message) => {
                                // Filter out messages from ourselves (self-echo)
                                let my_id = agent_id_clone.lock().await;
                                let is_self = my_id
                                    .as_ref()
                                    .map(|id| message.from == *id)
                                    .unwrap_or(false);
                                drop(my_id);

                                if !is_self {
                                    let my_id = agent_id_clone.lock().await;
                                    let id_str = my_id.clone().unwrap_or_default();
                                    drop(my_id);
                                    let mut inbox = inbox_clone.lock().await;
                                    inbox.push(message.clone());
                                    write_inbox_signal(&id_str, &inbox);
                                    drop(inbox);
                                }
                            }
                            MeshOperation::DiscoverResult(_) | MeshOperation::StatusResult(_) => {
                                let _ = response_tx.send(op);
                            }
                            MeshOperation::Error(_) => {
                                let _ = response_tx.send(op);
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        // Spawn heartbeat task
        let heartbeat_tx = broker_tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let op = MeshOperation::Heartbeat;
                if let Ok(json) = serde_json::to_string(&op) {
                    if heartbeat_tx.send(json).is_err() {
                        break;
                    }
                }
            }
        });

        // Store the connection
        {
            let mut conn = self.connection.lock().await;
            *conn = Some(BrokerConnection { tx: broker_tx });
        }
        {
            let mut rx = self.response_rx.lock().await;
            *rx = Some(response_rx);
        }

        Ok(())
    }

    /// Send a message to the broker. Ensures connection first.
    async fn send_to_broker(&self, msg: &str) -> Result<()> {
        self.ensure_connected().await?;
        let conn = self.connection.lock().await;
        if let Some(c) = conn.as_ref() {
            c.tx.send(msg.to_string())?;
        }
        Ok(())
    }

    pub async fn handle_message(&self, msg: &MCPMessage) -> Result<Option<serde_json::Value>> {
        let response = match msg.method.as_str() {
            "initialize" => self.handle_initialize().await?,
            "notifications/initialized" => return Ok(None),
            "notifications/tools/list" | "tools/list" => self.handle_tools_list().await?,
            "notifications/tools/call" | "tools/call" => {
                let params = msg
                    .params
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Missing params for tools/call"))?;
                let call: ToolCall = serde_json::from_value(params.clone())?;
                self.handle_tool_call(&call.name, &call.arguments).await?
            }
            _ => return Ok(None),
        };

        if let Some(id) = &msg.id {
            let mut full_response = response;
            if let Some(obj) = full_response.as_object_mut() {
                obj.insert("id".to_string(), id.clone());
            }
            Ok(Some(full_response))
        } else {
            Ok(None)
        }
    }

    async fn handle_initialize(&self) -> Result<serde_json::Value> {
        Ok(json!({
            "jsonrpc": "2.0",
            "result": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": SERVER_VERSION
                }
            }
        }))
    }

    async fn handle_tools_list(&self) -> Result<serde_json::Value> {
        let mut all_tools: Vec<serde_json::Value> = tools::mesh_tools()
            .into_iter()
            .map(|t| serde_json::to_value(t).unwrap())
            .collect();

        // Add dynamic inbox tools for each pending message
        let inbox = self.inbox.lock().await;
        for (i, msg) in inbox.iter().enumerate() {
            let truncated = if msg.content.text.len() > 300 {
                format!("{}...", &msg.content.text[..300])
            } else {
                msg.content.text.clone()
            };

            let label = match msg.msg_type {
                MessageType::Ask => format!(
                    "INCOMING QUESTION from '{}': \"{}\"\n\nCall this tool with your response to answer them.",
                    msg.from, truncated
                ),
                MessageType::Response => format!(
                    "RESPONSE from '{}': \"{}\"",
                    msg.from, truncated
                ),
                MessageType::System => format!(
                    "SYSTEM: \"{}\"",
                    truncated
                ),
                _ => format!("MESSAGE from '{}': \"{}\"", msg.from, truncated),
            };

            let schema = if msg.msg_type == MessageType::Ask {
                json!({
                    "type": "object",
                    "properties": {
                        "response": {
                            "type": "string",
                            "description": "Your response to the question above"
                        }
                    },
                    "required": ["response"]
                })
            } else {
                json!({
                    "type": "object",
                    "properties": {
                        "acknowledge": {
                            "type": "boolean",
                            "description": "Set to true to acknowledge and dismiss this message",
                            "default": true
                        }
                    }
                })
            };

            let tool = json!({
                "name": format!("mesh_inbox_{}", i),
                "description": label,
                "inputSchema": schema
            });
            all_tools.push(tool);
        }

        Ok(json!({
            "jsonrpc": "2.0",
            "result": {
                "tools": all_tools
            }
        }))
    }

    async fn handle_tool_call(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        if let Some(idx_str) = name.strip_prefix("mesh_inbox_") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                return self.tool_inbox_respond(idx, args).await;
            }
        }

        match name {
            "mesh_register" => self.tool_register(args).await,
            "mesh_discover" => self.tool_discover(args).await,
            "mesh_ask" => self.tool_ask(args).await,
            "mesh_check_messages" => self.tool_check_messages(args).await,
            "mesh_respond" => self.tool_respond(args).await,
            "mesh_status" => self.tool_status(args).await,
            _ => Ok(json!({
                "jsonrpc": "2.0",
                "result": {
                    "content": [{
                        "type": "text",
                        "text": format!("Unknown tool: {}", name)
                    }],
                    "isError": true
                }
            })),
        }
    }

    // --- Tool Implementations ---

    async fn tool_register(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        // Ensure broker connection before registering
        if let Err(e) = self.ensure_connected().await {
            return self.error_result(&format!("Could not connect to broker: {}", e));
        }

        let name = args["name"].as_str().unwrap_or("unnamed").to_string();
        let project = args["project"].as_str().unwrap_or("unknown").to_string();
        let project_path = args["project_path"].as_str().map(|s| s.to_string());
        let capabilities = args["capabilities"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| {
                        serde_json::from_value(json!(s)).unwrap_or(
                            crate::protocol::identity::AgentCapability::Custom(s.to_string()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        let payload = RegisterPayload {
            name,
            project,
            project_path,
            platform: Some(AgentPlatform::ClaudeCode),
            capabilities,
        };

        let op = MeshOperation::Register(payload);
        let json_str = serde_json::to_string(&op)?;
        self.send_to_broker(&json_str).await?;

        let response = self.wait_for_response(Duration::from_secs(5)).await;

        match response {
            Some(MeshOperation::Registered(payload)) => {
                let home = dirs_next::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                let watcher_path = home.join(".agentmesh").join("watch_inbox.sh");
                let watcher_hint = if watcher_path.exists() {
                    format!(
                        "\n\nIMPORTANT: You MUST now start the inbox watcher by running this command in the background:\n  bash ~/.agentmesh/watch_inbox.sh {} {}\nThis allows you to receive messages from other agents in real-time. Use run_in_background=true when running it.",
                        payload.agent_id, self.watcher_timeout_secs
                    )
                } else {
                    String::new()
                };
                self.text_result(&format!(
                    "Registered with AgentMesh broker. Agent ID: {}{}",
                    payload.agent_id, watcher_hint
                ))
            }
            Some(MeshOperation::Error(e)) => self.error_result(&e.message),
            _ => self.error_result("Timeout waiting for broker registration response"),
        }
    }

    async fn tool_discover(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        if let Err(e) = self.ensure_connected().await {
            return self.error_result(&format!("Could not connect to broker: {}", e));
        }

        let project = args["project"].as_str().map(|s| s.to_string());
        let online_only = args["online_only"].as_bool().unwrap_or(true);

        let payload = DiscoverPayload {
            project,
            capability: None,
            platform: None,
            online_only,
        };

        let op = MeshOperation::Discover(payload);
        let json_str = serde_json::to_string(&op)?;
        self.send_to_broker(&json_str).await?;

        let response = self.wait_for_response(Duration::from_secs(5)).await;

        match response {
            Some(MeshOperation::DiscoverResult(payload)) => {
                if payload.agents.is_empty() {
                    self.text_result("No agents found on the network.")
                } else {
                    let mut text = format!("Found {} agent(s):\n\n", payload.agents.len());
                    for agent in &payload.agents {
                        text.push_str(&format!(
                            "- **{}** (project: {}, platform: {:?}, status: {:?})\n  ID: {}\n",
                            agent.name, agent.project, agent.platform, agent.status, agent.agent_id
                        ));
                    }
                    self.text_result(&text)
                }
            }
            Some(MeshOperation::Error(e)) => self.error_result(&e.message),
            _ => self.error_result("Timeout waiting for discover response"),
        }
    }

    async fn tool_ask(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        if let Err(e) = self.ensure_connected().await {
            return self.error_result(&format!("Could not connect to broker: {}", e));
        }

        let to = args["to"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'to' parameter"))?;
        let question = args["question"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'question' parameter"))?;

        let agent_id = self.agent_id.lock().await;
        let from = agent_id.as_deref().unwrap_or("unregistered").to_string();
        drop(agent_id);

        let mut message = MeshMessage::ask_with_ttl(&from, to, question, self.message_ttl_secs);

        if let Some(data) = args.get("data") {
            if !data.is_null() {
                message.content.data = Some(data.clone());
            }
        }

        let op = MeshOperation::Send(message);
        let json_str = serde_json::to_string(&op)?;
        self.send_to_broker(&json_str).await?;

        self.text_result(&format!(
            "Question sent to '{}'. The response will appear when the agent replies. Use mesh_check_messages to check.",
            to
        ))
    }

    async fn tool_check_messages(&self, _args: &serde_json::Value) -> Result<serde_json::Value> {
        let agent_id = self.agent_id.lock().await;
        let id_str = agent_id.clone().unwrap_or_default();
        drop(agent_id);
        let mut inbox = self.inbox.lock().await;
        inbox.retain(|m| !m.is_expired());
        write_inbox_signal(&id_str, &inbox);

        if inbox.is_empty() {
            return self.text_result("No new messages in your inbox.");
        }

        let count = inbox.len();
        let mut text = format!("{} message(s) in inbox:\n\n", count);
        for (i, msg) in inbox.iter().enumerate() {
            text.push_str(&format!(
                "---\n**[{}]** From: {} | Type: {:?} | ID: {}\n**Message:** {}\n**Time:** {}\n",
                i, msg.from, msg.msg_type, msg.id, msg.content.text, msg.timestamp
            ));
        }

        text.push_str(
            "\nUse mesh_respond with the message ID, or call mesh_inbox_N to respond/acknowledge.",
        );
        self.text_result(&text)
    }

    async fn tool_inbox_respond(
        &self,
        idx: usize,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let mut inbox = self.inbox.lock().await;

        if idx >= inbox.len() {
            return self.error_result(&format!(
                "Inbox index {} out of range. You have {} message(s).",
                idx,
                inbox.len()
            ));
        }

        let original = inbox.remove(idx);
        let agent_id = self.agent_id.lock().await;
        let id_str = agent_id.clone().unwrap_or_default();
        drop(agent_id);
        write_inbox_signal(&id_str, &inbox);
        drop(inbox);

        if original.msg_type == MessageType::Ask {
            let response_text = args["response"].as_str().unwrap_or("(acknowledged)");

            let agent_id = self.agent_id.lock().await;
            let from = agent_id.as_deref().unwrap_or("unregistered").to_string();
            drop(agent_id);

            let response_msg = MeshMessage::response_with_ttl(
                &from,
                &original,
                response_text,
                self.message_ttl_secs,
            );
            let op = MeshOperation::Send(response_msg);
            let json_str = serde_json::to_string(&op)?;
            self.send_to_broker(&json_str).await?;
        }

        match original.msg_type {
            MessageType::Ask => {
                self.text_result(&format!("Response sent to agent '{}'.", original.from))
            }
            MessageType::Response => self.text_result(&format!(
                "Response from '{}': {}",
                original.from, original.content.text
            )),
            _ => self.text_result(&format!("Message acknowledged: {}", original.content.text)),
        }
    }

    async fn tool_respond(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let message_id = args["message_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'message_id' parameter"))?;
        let response_text = args["response"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'response' parameter"))?;

        let mut inbox = self.inbox.lock().await;
        let original_idx = inbox.iter().position(|m| m.id == message_id);

        let Some(idx) = original_idx else {
            return self.error_result(&format!(
                "Message '{}' not found in inbox. It may have expired.",
                message_id
            ));
        };

        let original = inbox.remove(idx);
        let agent_id = self.agent_id.lock().await;
        let id_str = agent_id.clone().unwrap_or_default();
        let from = agent_id.as_deref().unwrap_or("unregistered").to_string();
        drop(agent_id);
        write_inbox_signal(&id_str, &inbox);
        drop(inbox);

        let response_msg =
            MeshMessage::response_with_ttl(&from, &original, response_text, self.message_ttl_secs);
        let op = MeshOperation::Send(response_msg);
        let json_str = serde_json::to_string(&op)?;
        self.send_to_broker(&json_str).await?;

        self.text_result(&format!("Response sent to agent '{}'.", original.from))
    }

    async fn tool_status(&self, _args: &serde_json::Value) -> Result<serde_json::Value> {
        if let Err(e) = self.ensure_connected().await {
            return self.error_result(&format!("Could not connect to broker: {}", e));
        }

        let op = MeshOperation::Status;
        let json_str = serde_json::to_string(&op)?;
        self.send_to_broker(&json_str).await?;

        let response = self.wait_for_response(Duration::from_secs(5)).await;

        match response {
            Some(MeshOperation::StatusResult(payload)) => {
                let text = format!(
                    "AgentMesh Broker v{}\n\
                     Uptime: {}s\n\
                     Connected agents: {}\n\
                     Messages routed: {}\n\
                     Pending messages: {}",
                    payload.broker_version,
                    payload.uptime_secs,
                    payload.connected_agents,
                    payload.total_messages_routed,
                    payload.pending_messages,
                );
                self.text_result(&text)
            }
            Some(MeshOperation::Error(e)) => self.error_result(&e.message),
            _ => self.error_result("Timeout waiting for status response"),
        }
    }

    // --- Helpers ---

    async fn wait_for_response(&self, timeout: Duration) -> Option<MeshOperation> {
        let mut rx_guard = self.response_rx.lock().await;
        if let Some(rx) = rx_guard.as_mut() {
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(Some(op)) => Some(op),
                _ => None,
            }
        } else {
            None
        }
    }

    fn text_result(&self, text: &str) -> Result<serde_json::Value> {
        Ok(json!({
            "jsonrpc": "2.0",
            "result": {
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }
        }))
    }

    fn error_result(&self, message: &str) -> Result<serde_json::Value> {
        Ok(json!({
            "jsonrpc": "2.0",
            "result": {
                "content": [{
                    "type": "text",
                    "text": message
                }],
                "isError": true
            }
        }))
    }
}
