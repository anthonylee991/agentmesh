pub mod anthropic;
pub mod openai;
pub mod openrouter;

use anyhow::Result;
use async_trait::async_trait;

pub struct LlmRequest {
    pub prompt: String,
    pub system: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, request: &LlmRequest) -> Result<String>;
    fn name(&self) -> &str;
}

/// Create an LLM provider from config strings.
pub fn create_provider(
    provider: &str,
    api_key: &str,
    model: &str,
) -> Result<Box<dyn LlmProvider>> {
    match provider {
        "anthropic" => Ok(Box::new(anthropic::AnthropicProvider::new(api_key, model))),
        "openai" => Ok(Box::new(openai::OpenAIProvider::new(api_key, model))),
        "openrouter" => Ok(Box::new(openrouter::OpenRouterProvider::new(api_key, model))),
        _ => anyhow::bail!("Unknown LLM provider: {}", provider),
    }
}
