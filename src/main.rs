use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "agentmesh", about = "AgentMesh - Inter-agent messaging system")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the AgentMesh broker
    Broker {
        /// Port to listen on
        #[arg(long, default_value_t = 7777)]
        port: u16,
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    /// Start the MCP server (for Claude Code integration)
    Mcp {
        /// Broker WebSocket URL to connect to
        #[arg(long, default_value = "ws://127.0.0.1:7777/ws")]
        broker_url: String,
    },
    /// Show broker status
    Status {
        /// Broker HTTP URL
        #[arg(long, default_value = "http://127.0.0.1:7777")]
        broker_url: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Only init tracing for non-MCP modes (MCP uses stdio, tracing would corrupt it)
    let is_mcp = std::env::args().nth(1).as_deref() == Some("mcp");

    if !is_mcp {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "agentmesh=info".into()),
            )
            .init();
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Broker { port, host } => {
            let mut config = agentmesh::config::AppConfig::load().unwrap_or_default();
            config.broker.port = port;
            config.broker.host = host;
            agentmesh::broker::server::start_broker(config).await?;
        }
        Commands::Mcp { broker_url } => {
            // Create the server without connecting — it connects lazily on first tool call.
            // This means the MCP server starts instantly and never fails on startup.
            let server = agentmesh::mcp::server::MeshMCPServer::new(&broker_url);
            agentmesh::mcp::stdio::run_stdio(server).await?;
        }
        Commands::Status { broker_url } => {
            match reqwest::get(format!("{}/api/status", broker_url)).await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let body: serde_json::Value = resp.json().await?;
                        println!("{}", serde_json::to_string_pretty(&body)?);
                    } else {
                        eprintln!("Broker returned status: {}", resp.status());
                    }
                }
                Err(e) => {
                    eprintln!("Could not connect to broker at {}: {}", broker_url, e);
                    eprintln!("Is the broker running? Start it with: agentmesh broker");
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}
