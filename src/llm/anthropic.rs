use anyhow::Result;
use async_trait::async_trait;

use super::{LlmProvider, LlmRequest};

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl AnthropicProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn complete(&self, request: &LlmRequest) -> Result<String> {
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "messages": [{"role": "user", "content": request.prompt}]
        });

        if let Some(system) = &request.system {
            body["system"] = serde_json::Value::String(system.clone());
        }

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let body: serde_json::Value = response.json().await?;

        if !status.is_success() {
            let error_msg = body["error"]["message"]
                .as_str()
                .unwrap_or("Unknown API error");
            anyhow::bail!("Anthropic API error ({}): {}", status, error_msg);
        }

        let text = body["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(text)
    }

    fn name(&self) -> &str {
        "anthropic"
    }
}
