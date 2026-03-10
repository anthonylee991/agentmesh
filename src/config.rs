use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_BROKER_PORT: u16 = 7777;
const DEFAULT_BROKER_HOST: &str = "127.0.0.1";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub broker: BrokerConfig,
    #[serde(default)]
    pub proxy: ProxyConfig,
    #[serde(default)]
    pub pro: ProConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_message_ttl")]
    pub message_ttl_secs: u32,
    #[serde(default = "default_watcher_timeout")]
    pub watcher_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_timeout")]
    pub live_agent_timeout: u64,
    #[serde(default)]
    pub api_keys: ApiKeys,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiKeys {
    #[serde(default)]
    pub anthropic: Option<String>,
    #[serde(default)]
    pub openai: Option<String>,
    #[serde(default)]
    pub openrouter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProConfig {
    #[serde(default)]
    pub license_key: Option<String>,
    #[serde(default)]
    pub relay_url: Option<String>,
}

/// Per-project config stored in `<project>/.agentmesh/config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default)]
    pub project: ProjectInfo,
    #[serde(default)]
    pub proxy: ProjectProxyConfig,
    #[serde(default)]
    pub agent: ProjectAgentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectInfo {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectProxyConfig {
    #[serde(default = "default_context_files")]
    pub context_files: Vec<String>,
    #[serde(default = "default_max_context")]
    pub max_context_chars: usize,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectAgentConfig {
    #[serde(default)]
    pub capabilities: Vec<String>,
}

fn default_port() -> u16 {
    DEFAULT_BROKER_PORT
}
fn default_host() -> String {
    DEFAULT_BROKER_HOST.to_string()
}
fn default_provider() -> String {
    "anthropic".to_string()
}
fn default_model() -> String {
    "claude-sonnet-4-20250514".to_string()
}
fn default_timeout() -> u64 {
    120
}
fn default_message_ttl() -> u32 {
    3600
}
fn default_watcher_timeout() -> u64 {
    7200
}
fn default_context_files() -> Vec<String> {
    vec![
        "CLAUDE.md".to_string(),
        "README.md".to_string(),
        "Cargo.toml".to_string(),
        "package.json".to_string(),
    ]
}
fn default_max_context() -> usize {
    12000
}

impl Default for BrokerConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
            message_ttl_secs: default_message_ttl(),
            watcher_timeout_secs: default_watcher_timeout(),
        }
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            live_agent_timeout: default_timeout(),
            api_keys: ApiKeys::default(),
        }
    }
}

impl Default for ProjectProxyConfig {
    fn default() -> Self {
        Self {
            context_files: default_context_files(),
            max_context_chars: default_max_context(),
            provider: None,
            model: None,
        }
    }
}

impl AppConfig {
    /// Load global config from `~/.agentmesh/config.toml`, falling back to defaults.
    pub fn load() -> Result<Self> {
        let config_path = Self::global_config_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: AppConfig = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    pub fn global_config_path() -> PathBuf {
        let home = dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".agentmesh").join("config.toml")
    }

    pub fn broker_url(&self) -> String {
        format!("ws://{}:{}/ws", self.broker.host, self.broker.port)
    }

    pub fn broker_http_url(&self) -> String {
        format!("http://{}:{}", self.broker.host, self.broker.port)
    }
}

impl ProjectConfig {
    /// Load project config from `<project_path>/.agentmesh/config.toml`.
    pub fn load(project_path: &str) -> Result<Self> {
        let config_path = PathBuf::from(project_path)
            .join(".agentmesh")
            .join("config.toml");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: ProjectConfig = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }
}
