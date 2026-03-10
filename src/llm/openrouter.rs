use anyhow::Result;
use async_trait::async_trait;

use super::{LlmProvider, LlmRequest};

pub struct OpenRouterProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl OpenRouterProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    async fn complete(&self, request: &LlmRequest) -> Result<String> {
        let mut messages = vec![];

        if let Some(system) = &request.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        messages.push(serde_json::json!({"role": "user", "content": request.prompt}));

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "messages": messages,
        });

        let response = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .header("HTTP-Referer", "https://agentmesh.dev")
            .header("X-Title", "AgentMesh Proxy")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let body: serde_json::Value = response.json().await?;

        if !status.is_success() {
            let error_msg = body["error"]["message"]
                .as_str()
                .unwrap_or("Unknown API error");
            anyhow::bail!("OpenRouter API error ({}): {}", status, error_msg);
        }

        let text = body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(text)
    }

    fn name(&self) -> &str {
        "openrouter"
    }
}
