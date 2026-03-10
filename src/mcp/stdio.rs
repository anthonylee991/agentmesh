use anyhow::Result;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

use super::server::{MCPMessage, MeshMCPServer};

/// Run the MCP server in stdio mode (for Claude Code).
/// Reads JSON-RPC messages from stdin, dispatches to server, writes responses to stdout.
pub async fn run_stdio(server: MeshMCPServer) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut writer = BufWriter::new(tokio::io::stdout());

    while let Some(line) = reader.next_line().await? {
        if line.is_empty() {
            continue;
        }

        let msg: MCPMessage = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[agentmesh-mcp] Failed to parse message: {}", e);
                continue;
            }
        };

        match server.handle_message(&msg).await {
            Ok(Some(response)) => {
                let response_str = serde_json::to_string(&response).unwrap();
                writer.write_all(response_str.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("[agentmesh-mcp] Error handling message: {}", e);
                if let Some(id) = &msg.id {
                    let error_response = json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32603,
                            "message": e.to_string()
                        }
                    });
                    if let Ok(response_str) = serde_json::to_string(&error_response) {
                        let _ = writer.write_all(response_str.as_bytes()).await;
                        let _ = writer.write_all(b"\n").await;
                        let _ = writer.flush().await;
                    }
                }
            }
        }
    }

    Ok(())
}
