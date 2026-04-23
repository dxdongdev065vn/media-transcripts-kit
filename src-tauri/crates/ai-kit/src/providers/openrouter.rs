//! OpenRouter provider.
//!
//! Wraps the OpenAI-compatible API at `https://openrouter.ai/api/v1`.
//! Reuses `OpenAiProvider::build_body` / `parse_response` — no duplication.
//! Adds the `HTTP-Referer` and `X-Title` attribution headers OpenRouter
//! recommends for routing and analytics.

use async_trait::async_trait;
use serde_json::Value;

use creator_core::{AiProviderError, AiProviderType};

use crate::request::CompletionRequest;
use crate::providers::openai::OpenAiProvider;
use crate::{Provider, ProviderCapability};

pub const OPENROUTER_API_BASE: &str = "https://openrouter.ai/api/v1";

pub struct OpenRouterProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenRouterProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: OPENROUTER_API_BASE.into(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    fn provider_type(&self) -> AiProviderType {
        AiProviderType::OpenRouter
    }

    fn supports(&self, capability: ProviderCapability) -> bool {
        matches!(capability, ProviderCapability::Text | ProviderCapability::Vision)
    }

    async fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    async fn health_check(&self) -> Result<(), AiProviderError> {
        if self.api_key.is_empty() {
            return Err(AiProviderError::MissingApiKey(AiProviderType::OpenRouter));
        }
        let url = format!("{}/models", self.base_url);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", "https://github.com/phuc-nt/my-media-kit")
            .header("X-Title", "MyMediaKit")
            .send()
            .await
            .map_err(|e| AiProviderError::Network(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            Err(AiProviderError::Rejected(format!("{status}: {text}")))
        }
    }

    async fn complete(&self, request: CompletionRequest) -> Result<Value, AiProviderError> {
        if self.api_key.is_empty() {
            return Err(AiProviderError::MissingApiKey(AiProviderType::OpenRouter));
        }
        let url = format!("{}/chat/completions", self.base_url);
        let body = OpenAiProvider::build_body(&request);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .header("HTTP-Referer", "https://github.com/phuc-nt/my-media-kit")
            .header("X-Title", "MyMediaKit")
            .json(&body)
            .send()
            .await
            .map_err(|e| AiProviderError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AiProviderError::Rejected(format!("{status}: {text}")));
        }
        let json: Value = resp
            .json()
            .await
            .map_err(|e| AiProviderError::Malformed(e.to_string()))?;
        OpenAiProvider::parse_response(&request, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::CompletionRequest;

    #[test]
    fn provider_type_is_openrouter() {
        let p = OpenRouterProvider::new("sk-or-test");
        assert_eq!(p.provider_type(), AiProviderType::OpenRouter);
    }

    #[test]
    fn unavailable_when_empty_key() {
        let p = OpenRouterProvider::new("");
        // is_available is async; call via block_on in test
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert!(!rt.block_on(p.is_available()));
    }

    #[test]
    fn body_delegates_to_openai_builder() {
        let req = CompletionRequest::freeform("anthropic/claude-3-5-sonnet", "sys", "usr");
        let body = OpenAiProvider::build_body(&req);
        assert_eq!(body["model"], "anthropic/claude-3-5-sonnet");
        assert!(body.get("max_completion_tokens").is_some());
    }
}
