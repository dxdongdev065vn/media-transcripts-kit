use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};

use creator_core::{AiProviderError, AiProviderType};

use crate::config::{ProviderCapability, ProviderConfig};
use crate::{CompletionRequest, Provider};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AIRequestContext {
    pub tool_name: String,
    pub capability: ProviderCapability,
    #[serde(default)]
    pub preferred_provider_id: Option<String>,
    #[serde(default)]
    pub preferred_provider: Option<AiProviderType>,
    #[serde(default)]
    pub preferred_model: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    #[serde(default = "default_attempts_per_provider")]
    pub max_attempts_per_provider: u32,
    #[serde(default = "default_retry_backoff_ms")]
    pub retry_backoff_ms: u64,
}

fn default_attempts_per_provider() -> u32 {
    2
}

fn default_retry_backoff_ms() -> u64 {
    1200
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayAttempt {
    pub provider_id: String,
    pub provider_label: String,
    pub provider_type: AiProviderType,
    pub model: String,
    pub duration_ms: u128,
    pub success: bool,
    #[serde(default)]
    pub error: Option<String>,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCompletion {
    pub value: Value,
    pub provider_id: String,
    pub provider_label: String,
    pub provider_type: AiProviderType,
    pub model: String,
    pub attempts: Vec<GatewayAttempt>,
}

pub async fn invoke_with_fallback<F>(
    configs: &[ProviderConfig],
    context: &AIRequestContext,
    request: CompletionRequest,
    mut resolver: F,
) -> Result<GatewayCompletion, AiProviderError>
where
    F: FnMut(&ProviderConfig) -> Result<Arc<dyn Provider>, AiProviderError>,
{
    let mut candidates = configs
        .iter()
        .filter(|cfg| cfg.enabled && cfg.supports(context.capability))
        .cloned()
        .collect::<Vec<_>>();
    candidates.sort_by_key(|cfg| {
        let preferred_id = context
            .preferred_provider_id
            .as_ref()
            .map(|id| (cfg.id != *id) as u8)
            .unwrap_or(0);
        let preferred = context
            .preferred_provider
            .map(|kind| (cfg.provider_type != kind) as u8)
            .unwrap_or(0);
        (preferred_id, preferred, cfg.priority)
    });

    if candidates.is_empty() {
        return Err(AiProviderError::Rejected(format!(
            "no enabled providers support {:?} for {}",
            context.capability, context.tool_name
        )));
    }

    let mut attempts = Vec::new();
    let mut failures = Vec::new();

    for cfg in candidates {
        let provider = match resolver(&cfg) {
            Ok(provider) => provider,
            Err(err) => {
                failures.push(format!("{}: {}", cfg.label, err));
                attempts.push(GatewayAttempt {
                    provider_id: cfg.id.clone(),
                    provider_label: cfg.label.clone(),
                    provider_type: cfg.provider_type,
                    model: pick_model(context, &request, &cfg),
                    duration_ms: 0,
                    success: false,
                    error: Some(err.to_string()),
                    retryable: is_retryable(&err),
                });
                continue;
            }
        };

        let model = pick_model(context, &request, &cfg);
        let mut request_for_provider = request.clone();
        request_for_provider.model = model.clone();

        let max_attempts = context.max_attempts_per_provider.max(1);
        for attempt_index in 0..max_attempts {
            let started = Instant::now();
            let request_for_attempt = request_for_provider.clone();
            let outcome = if let Some(timeout_ms) = context.timeout_ms {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(timeout_ms),
                    provider.complete(request_for_attempt),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err(AiProviderError::Network(format!(
                        "request timed out after {timeout_ms}ms"
                    ))),
                }
            } else {
                provider.complete(request_for_attempt).await
            };
            let duration_ms = started.elapsed().as_millis();

            match outcome {
                Ok(value) => {
                    info!(
                        tool = %context.tool_name,
                        provider_id = %cfg.id,
                        provider_type = ?cfg.provider_type,
                        duration_ms = duration_ms,
                        attempts = attempts.len() + 1,
                        provider_attempt = attempt_index + 1,
                        "ai request succeeded"
                    );
                    attempts.push(GatewayAttempt {
                        provider_id: cfg.id.clone(),
                        provider_label: cfg.label.clone(),
                        provider_type: cfg.provider_type,
                        model: model.clone(),
                        duration_ms,
                        success: true,
                        error: None,
                        retryable: false,
                    });
                    return Ok(GatewayCompletion {
                        value,
                        provider_id: cfg.id,
                        provider_label: cfg.label,
                        provider_type: cfg.provider_type,
                        model,
                        attempts,
                    });
                }
                Err(err) => {
                    let retryable = is_retryable(&err);
                    warn!(
                        tool = %context.tool_name,
                        provider_id = %cfg.id,
                        provider_type = ?cfg.provider_type,
                        duration_ms = duration_ms,
                        retryable = retryable,
                        provider_attempt = attempt_index + 1,
                        error = %err,
                        "ai request failed"
                    );
                    failures.push(format!("{}: {}", cfg.label, err));
                    attempts.push(GatewayAttempt {
                        provider_id: cfg.id.clone(),
                        provider_label: cfg.label.clone(),
                        provider_type: cfg.provider_type,
                        model: model.clone(),
                        duration_ms,
                        success: false,
                        error: Some(err.to_string()),
                        retryable,
                    });
                    let should_retry_same_provider =
                        retryable && (attempt_index + 1) < max_attempts;
                    if should_retry_same_provider {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            context.retry_backoff_ms.max(1) * u64::from(attempt_index + 1),
                        ))
                        .await;
                        continue;
                    }
                    break;
                }
            }
        }
    }

    Err(AiProviderError::Rejected(format!(
        "all providers failed for {}: {}",
        context.tool_name,
        failures.join(" | ")
    )))
}

fn pick_model(
    context: &AIRequestContext,
    request: &CompletionRequest,
    config: &ProviderConfig,
) -> String {
    context
        .preferred_model
        .clone()
        .filter(|model| !model.trim().is_empty())
        .or_else(|| {
            (!request.model.trim().is_empty())
                .then(|| request.model.clone())
                .filter(|model| !model.trim().is_empty())
        })
        .unwrap_or_else(|| config.default_model.clone())
}

fn is_retryable(error: &AiProviderError) -> bool {
    match error {
        AiProviderError::Network(_) | AiProviderError::Cancelled => true,
        AiProviderError::Rejected(message) => {
            let lower = message.to_ascii_lowercase();
            lower.contains("429")
                || lower.contains("500")
                || lower.contains("502")
                || lower.contains("503")
                || lower.contains("504")
                || lower.contains("rate limit")
                || lower.contains("quota")
                || lower.contains("temporar")
                || lower.contains("model unavailable")
                || lower.contains("overloaded")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;

    struct StubProvider {
        response: Mutex<Option<Result<Value, AiProviderError>>>,
    }

    #[async_trait]
    impl Provider for StubProvider {
        fn provider_type(&self) -> AiProviderType {
            AiProviderType::OpenAi
        }

        async fn is_available(&self) -> bool {
            true
        }

        async fn complete(&self, _request: CompletionRequest) -> Result<Value, AiProviderError> {
            self.response
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| Ok(json!({"text":"ok"})))
        }
    }

    #[tokio::test]
    async fn falls_back_to_second_provider_after_retryable_error() {
        let configs = vec![
            ProviderConfig {
                id: "one".into(),
                label: "One".into(),
                provider_type: AiProviderType::OpenAi,
                base_url: None,
                default_model: "a".into(),
                enabled: true,
                priority: 0,
                capabilities: vec![ProviderCapability::Text],
                api_key_ref: None,
            },
            ProviderConfig {
                id: "two".into(),
                label: "Two".into(),
                provider_type: AiProviderType::OpenAi,
                base_url: None,
                default_model: "b".into(),
                enabled: true,
                priority: 1,
                capabilities: vec![ProviderCapability::Text],
                api_key_ref: None,
            },
        ];
        let ctx = AIRequestContext {
            tool_name: "summary".into(),
            capability: ProviderCapability::Text,
            preferred_provider_id: None,
            preferred_provider: None,
            preferred_model: None,
            timeout_ms: None,
            metadata: HashMap::new(),
            max_attempts_per_provider: 1,
            retry_backoff_ms: 1,
        };
        let req = CompletionRequest::freeform("", "sys", "usr");
        let result = invoke_with_fallback(&configs, &ctx, req, |cfg| {
            if cfg.id == "one" {
                Ok(Arc::new(StubProvider {
                    response: Mutex::new(Some(Err(AiProviderError::Rejected(
                        "429 rate limit".into(),
                    )))),
                }))
            } else {
                Ok(Arc::new(StubProvider {
                    response: Mutex::new(Some(Ok(json!({"text":"done"})))),
                }))
            }
        })
        .await
        .unwrap();
        assert_eq!(result.provider_id, "two");
        assert_eq!(result.attempts.len(), 2);
        assert!(result.attempts[0].retryable);
    }
}
