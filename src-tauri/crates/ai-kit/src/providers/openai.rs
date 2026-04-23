//! OpenAI chat completions provider.
//!
//! Uses `/v1/chat/completions` with `response_format={type: json_schema, ...}`
//! for structured output. `max_completion_tokens` replaces the older
//! `max_tokens` field per the 2025 API guidance.

use async_trait::async_trait;
use serde_json::{json, Value};

use creator_core::{AiProviderError, AiProviderType};

use crate::request::{CompletionRequest, ResponseFormat};
use crate::{Provider, ProviderCapability};

pub const OPENAI_API_BASE: &str = "https://api.openai.com";
pub const DEFAULT_MODEL: &str = "gpt-4o-mini";

pub struct OpenAiProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: OPENAI_API_BASE.to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base_url = base.into();
        self
    }

    fn normalized_base_url(&self) -> String {
        normalize_openai_base_url(&self.base_url)
    }

    pub fn build_body(request: &CompletionRequest) -> Value {
        let messages = json!([
            { "role": "system", "content": request.system },
            { "role": "user",   "content": request.user_prompt },
        ]);

        // Use `max_completion_tokens` (2025+ OpenAI API). The old `max_tokens`
        // field is no longer accepted alongside it — sending both causes a 400.
        // mlx_lm.server also accepts this field in recent versions.
        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "temperature": request.temperature,
            "max_completion_tokens": request.max_tokens,
        });

        if let ResponseFormat::JsonSchema { name, schema } = &request.response_format {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {
                    "name": name,
                    "schema": schema,
                    "strict": true,
                }
            });
        }

        body
    }

    pub fn parse_response(
        request: &CompletionRequest,
        body: &Value,
    ) -> Result<Value, AiProviderError> {
        let text = body
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| AiProviderError::Malformed("missing choices[0].message.content".into()))?;

        match &request.response_format {
            ResponseFormat::Freeform => Ok(json!({ "text": text })),
            ResponseFormat::JsonSchema { .. } => serde_json::from_str::<Value>(text)
                .map_err(|e| AiProviderError::Malformed(format!("json parse: {e}"))),
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn provider_type(&self) -> AiProviderType {
        AiProviderType::OpenAi
    }

    fn supports(&self, capability: ProviderCapability) -> bool {
        matches!(
            capability,
            ProviderCapability::Text | ProviderCapability::Vision | ProviderCapability::Transcription
        )
    }

    async fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    async fn health_check(&self) -> Result<(), AiProviderError> {
        if self.api_key.is_empty() {
            return Err(AiProviderError::MissingApiKey(AiProviderType::OpenAi));
        }
        let url = format!("{}/models", self.normalized_base_url());
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
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
            return Err(AiProviderError::MissingApiKey(AiProviderType::OpenAi));
        }
        let url = format!("{}/chat/completions", self.normalized_base_url());
        let body = Self::build_body(&request);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
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
        Self::parse_response(&request, &json)
    }
}

fn normalize_openai_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        return trimmed
            .trim_end_matches("/chat/completions")
            .trim_end_matches('/')
            .to_string();
    }
    if trimmed.ends_with("/models") {
        return trimmed.trim_end_matches("/models").trim_end_matches('/').to_string();
    }
    if trimmed.ends_with("/completions") {
        return trimmed
            .trim_end_matches("/completions")
            .trim_end_matches('/')
            .to_string();
    }
    if trimmed.ends_with("/v1") {
        return trimmed.to_string();
    }
    format!("{trimmed}/v1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_contains_messages_and_max_completion_tokens() {
        let req = CompletionRequest::freeform(DEFAULT_MODEL, "sys", "usr");
        let body = OpenAiProvider::build_body(&req);
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        assert!(body.get("max_completion_tokens").is_some());
    }

    #[test]
    fn structured_body_uses_json_schema_strict() {
        let req = CompletionRequest::structured(
            DEFAULT_MODEL,
            "s",
            "u",
            "Out",
            json!({"type":"object"}),
        );
        let body = OpenAiProvider::build_body(&req);
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["response_format"]["json_schema"]["name"], "Out");
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
    }

    #[test]
    fn parses_structured_content() {
        let req = CompletionRequest::structured(
            DEFAULT_MODEL,
            "s",
            "u",
            "Out",
            json!({"type":"object"}),
        );
        let body = json!({
            "choices": [{
                "message": {
                    "content": "{\"answer\": 7}"
                }
            }]
        });
        let out = OpenAiProvider::parse_response(&req, &body).unwrap();
        assert_eq!(out["answer"], 7);
    }

    #[test]
    fn parses_freeform_content() {
        let req = CompletionRequest::freeform(DEFAULT_MODEL, "s", "u");
        let body = json!({
            "choices": [{
                "message": { "content": "hello" }
            }]
        });
        let out = OpenAiProvider::parse_response(&req, &body).unwrap();
        assert_eq!(out["text"], "hello");
    }

    #[test]
    fn normalizes_full_chat_endpoint_to_v1_base() {
        assert_eq!(
            normalize_openai_base_url("https://platform.beeknoee.com/api/v1/chat/completions"),
            "https://platform.beeknoee.com/api/v1"
        );
    }

    #[test]
    fn appends_v1_when_missing() {
        assert_eq!(
            normalize_openai_base_url("https://platform.beeknoee.com/api"),
            "https://platform.beeknoee.com/api/v1"
        );
    }
}
