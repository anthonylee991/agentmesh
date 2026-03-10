use anyhow::Result;
use chrono::Utc;
use std::path::{Path, PathBuf};

use crate::config::{AppConfig, ProjectConfig};
use crate::llm::{self, LlmRequest};
use crate::protocol::message::{MeshMessage, MessageContent, MessageType};

/// Proxy agent that answers questions using project context + LLM.
/// Used as fallback when no live agent responds within the timeout.
pub struct ProxyAgent {
    config: AppConfig,
}

impl ProxyAgent {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    /// Generate a proxy response for an unanswered Ask message.
    pub async fn respond(
        &self,
        message: &MeshMessage,
        project_path: Option<&str>,
    ) -> Result<MeshMessage> {
        let project = message.project.as_deref().unwrap_or(&message.to);

        let context = match project_path {
            Some(path) => self.gather_context(path).await?,
            None => format!("No project files available for '{}'.", project),
        };

        // Load project-specific config if available
        let project_config = project_path
            .map(|p| ProjectConfig::load(p).unwrap_or_default())
            .unwrap_or_default();

        // Determine LLM provider: project override > global config
        let provider_name = project_config
            .proxy
            .provider
            .as_deref()
            .unwrap_or(&self.config.proxy.provider);

        let model = project_config
            .proxy
            .model
            .as_deref()
            .unwrap_or(&self.config.proxy.model);

        let api_key = self.get_api_key(provider_name)?;
        let provider = llm::create_provider(provider_name, &api_key, model)?;

        let system = format!(
            "You are a proxy AI assistant for the project '{project}'. \
             Another AI agent asked you a question. Answer based on the project context below. \
             If you cannot answer from the context, say so honestly. \
             Be concise and direct.",
        );

        let prompt = format!(
            "## Project Context\n\n{context}\n\n## Question\n\n{question}",
            context = context,
            question = message.content.text,
        );

        tracing::info!(
            project = %project,
            provider = %provider_name,
            model = %model,
            "Proxy agent generating response"
        );

        let response_text = provider
            .complete(&LlmRequest {
                prompt,
                system: Some(system),
                max_tokens: 2000,
                temperature: 0.3,
            })
            .await?;

        Ok(MeshMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from: format!("proxy:{}", project),
            to: message.from.clone(),
            msg_type: MessageType::Response,
            content: MessageContent {
                text: response_text,
                data: None,
                attachments: vec![],
            },
            project: message.project.clone(),
            correlation_id: Some(message.id.clone()),
            timestamp: Utc::now(),
            ttl: self.config.broker.message_ttl_secs,
            proxy_response: true,
        })
    }

    /// Gather project context by reading standard files from the project directory.
    async fn gather_context(&self, project_path: &str) -> Result<String> {
        let root = PathBuf::from(project_path);

        if !root.exists() {
            return Ok(format!(
                "Project directory '{}' does not exist.",
                project_path
            ));
        }

        // Load project config for custom context_files, or use defaults
        let project_config = ProjectConfig::load(project_path).unwrap_or_default();
        let max_chars = project_config.proxy.max_context_chars;

        let mut context = String::new();
        let mut total_chars = 0;

        // Read configured context files
        for file in &project_config.proxy.context_files {
            let file_path = root.join(file);
            if let Some(content) = self
                .read_file_safe(&file_path, max_chars - total_chars)
                .await
            {
                context.push_str(&format!("### {}\n```\n{}\n```\n\n", file, content));
                total_chars = context.len();
                if total_chars >= max_chars {
                    break;
                }
            }
        }

        // Also try .claude/CLAUDE.md if not already included
        let claude_md = root.join(".claude").join("CLAUDE.md");
        if claude_md.exists()
            && !project_config
                .proxy
                .context_files
                .iter()
                .any(|f| f.contains("CLAUDE.md"))
        {
            if let Some(content) = self
                .read_file_safe(&claude_md, max_chars - total_chars)
                .await
            {
                context.push_str(&format!("### .claude/CLAUDE.md\n```\n{}\n```\n\n", content));
                total_chars = context.len();
            }
        }

        // Try to get recent git log
        if total_chars < max_chars {
            if let Some(git_log) = self.get_git_log(&root, 20).await {
                context.push_str(&format!(
                    "### Recent Git History\n```\n{}\n```\n\n",
                    git_log
                ));
            }
        }

        if context.is_empty() {
            context = format!(
                "No readable files found in project directory '{}'.",
                project_path
            );
        }

        Ok(context)
    }

    /// Read a file, truncating to max_chars. Returns None if file doesn't exist or can't be read.
    async fn read_file_safe(&self, path: &Path, max_chars: usize) -> Option<String> {
        if max_chars == 0 {
            return None;
        }
        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                if content.len() > max_chars {
                    Some(format!(
                        "{}...\n[truncated, {} total chars]",
                        &content[..max_chars],
                        content.len()
                    ))
                } else {
                    Some(content)
                }
            }
            Err(_) => None,
        }
    }

    /// Run `git log --oneline -N` in the project directory.
    async fn get_git_log(&self, project_path: &Path, count: usize) -> Option<String> {
        let output = tokio::process::Command::new("git")
            .args(["log", "--oneline", &format!("-{}", count)])
            .current_dir(project_path)
            .output()
            .await
            .ok()?;

        if output.status.success() {
            let log = String::from_utf8_lossy(&output.stdout).to_string();
            if log.is_empty() {
                None
            } else {
                Some(log)
            }
        } else {
            None
        }
    }

    /// Get API key for the given provider from config or environment.
    fn get_api_key(&self, provider: &str) -> Result<String> {
        // Check config first
        let from_config = match provider {
            "anthropic" => self.config.proxy.api_keys.anthropic.as_deref(),
            "openai" => self.config.proxy.api_keys.openai.as_deref(),
            "openrouter" => self.config.proxy.api_keys.openrouter.as_deref(),
            _ => None,
        };

        if let Some(key) = from_config {
            if !key.is_empty() {
                return Ok(key.to_string());
            }
        }

        // Fall back to environment variables
        let env_var = match provider {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "openrouter" => "OPENROUTER_API_KEY",
            _ => return Err(anyhow::anyhow!("Unknown provider: {}", provider)),
        };

        std::env::var(env_var).map_err(|_| {
            anyhow::anyhow!(
                "No API key found for '{}'. Set it in ~/.agentmesh/config.toml or as {} env var.",
                provider,
                env_var
            )
        })
    }
}
