//! AI provider commands + provider settings persistence.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{command, AppHandle, Manager};
use uuid::Uuid;

use ai_kit::providers::ollama::DEFAULT_HOST;
use ai_kit::providers::openai::{DEFAULT_MODEL as OPENAI_DEFAULT_MODEL, OPENAI_API_BASE};
use ai_kit::providers::openrouter::OPENROUTER_API_BASE;
use ai_kit::{
    invoke_with_fallback, AIRequestContext, ClaudeProvider, CompletionRequest, GeminiProvider,
    KeyringSecretStore, OllamaProvider, OpenAiProvider, OpenRouterProvider, Provider,
    ProviderCapability, ProviderConfig, ProviderSettings, SecretStore,
};
use creator_core::{AiProviderError, AiProviderType};

const PROVIDER_SETTINGS_FILE: &str = "provider-settings.json";
const BEEKNOEE_API_BASE: &str = "https://platform.beeknoee.com/api/v1/chat/completions";
const BEEKNOEE_DEFAULT_MODEL: &str = "gpt-4o-mini";
const WORKSPACE_CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceConfigFile {
    #[serde(default)]
    ai: WorkspaceAiConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceAiConfig {
    #[serde(default)]
    retry: WorkspaceRetryConfig,
    #[serde(default)]
    default_text_provider_id: Option<String>,
    #[serde(default)]
    default_transcription_provider_id: Option<String>,
    #[serde(default)]
    providers: Vec<WorkspaceProviderConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceRetryConfig {
    #[serde(default)]
    max_attempts_per_provider: Option<u32>,
    #[serde(default)]
    retry_backoff_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceProviderConfig {
    id: String,
    label: String,
    provider_type: AiProviderType,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    default_model: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    priority: Option<u32>,
    #[serde(default)]
    capabilities: Vec<ProviderCapability>,
    #[serde(default)]
    api_key_ref: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    pub provider: AiProviderType,
    pub has_key: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigView {
    pub id: String,
    pub label: String,
    pub provider_type: AiProviderType,
    pub base_url: Option<String>,
    pub default_model: String,
    pub enabled: bool,
    pub priority: u32,
    pub capabilities: Vec<ProviderCapability>,
    pub api_key_ref: String,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsView {
    pub providers: Vec<ProviderConfigView>,
    pub default_text_provider_id: Option<String>,
    pub default_text_model: Option<String>,
    pub default_transcription_provider_id: Option<String>,
    pub default_transcription_model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatusView {
    pub id: String,
    pub provider: AiProviderType,
    pub display_name: String,
    pub enabled: bool,
    pub available: bool,
    pub priority: u32,
    pub default_model: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigInput {
    pub id: Option<String>,
    pub label: String,
    pub provider_type: AiProviderType,
    #[serde(default)]
    pub base_url: Option<String>,
    pub default_model: String,
    pub enabled: bool,
    pub priority: u32,
    #[serde(default)]
    pub capabilities: Vec<ProviderCapability>,
    #[serde(default)]
    pub api_key_ref: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsInput {
    #[serde(default)]
    pub default_text_provider_id: Option<String>,
    #[serde(default)]
    pub default_transcription_provider_id: Option<String>,
    pub providers: Vec<ProviderConfigInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestInput {
    pub provider: ProviderConfigInput,
    #[serde(default)]
    pub capability: Option<ProviderCapability>,
}

#[derive(Clone)]
pub(crate) struct ConfiguredGatewayProvider {
    settings: ProviderSettings,
    retry: WorkspaceRetryConfig,
    tool_name: String,
    capability: ProviderCapability,
    preferred_provider_id: Option<String>,
    preferred_provider: Option<AiProviderType>,
    timeout_ms: Option<u64>,
}

impl ConfiguredGatewayProvider {
    pub(crate) fn new(
        settings: ProviderSettings,
        retry: WorkspaceRetryConfig,
        tool_name: impl Into<String>,
        capability: ProviderCapability,
        preferred_provider_id: Option<String>,
        preferred_provider: Option<AiProviderType>,
    ) -> Self {
        Self {
            settings,
            retry,
            tool_name: tool_name.into(),
            capability,
            preferred_provider_id,
            preferred_provider,
            timeout_ms: Some(120_000),
        }
    }
}

#[async_trait::async_trait]
impl Provider for ConfiguredGatewayProvider {
    fn provider_type(&self) -> AiProviderType {
        self.preferred_provider.unwrap_or(AiProviderType::OpenAi)
    }

    fn supports(&self, capability: ProviderCapability) -> bool {
        capability == self.capability
    }

    async fn is_available(&self) -> bool {
        let store = KeyringSecretStore::new();
        self.settings
            .sorted_providers()
            .into_iter()
            .filter(|cfg| cfg.enabled && cfg.supports(self.capability))
            .any(|cfg| build_provider_from_config(&cfg, &store).is_ok())
    }

    async fn health_check(&self) -> Result<(), AiProviderError> {
        let store = KeyringSecretStore::new();
        for cfg in self.settings.sorted_providers() {
            if !cfg.enabled || !cfg.supports(self.capability) {
                continue;
            }
            let provider = build_provider_from_config(&cfg, &store)?;
            if provider.health_check().await.is_ok() {
                return Ok(());
            }
        }
        Err(AiProviderError::Rejected(format!(
            "no healthy providers available for {}",
            self.tool_name
        )))
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<serde_json::Value, AiProviderError> {
        let context = AIRequestContext {
            tool_name: self.tool_name.clone(),
            capability: self.capability,
            preferred_provider_id: self.preferred_provider_id.clone(),
            preferred_provider: self.preferred_provider,
            preferred_model: Some(request.model.clone()).filter(|m| !m.trim().is_empty()),
            timeout_ms: self.timeout_ms,
            metadata: std::collections::HashMap::new(),
            max_attempts_per_provider: self.retry.max_attempts_per_provider.unwrap_or(2),
            retry_backoff_ms: self.retry.retry_backoff_ms.unwrap_or(1200),
        };
        let store = KeyringSecretStore::new();
        let completion = invoke_with_fallback(
            &self.settings.sorted_providers(),
            &context,
            request,
            |cfg| build_provider_from_config(cfg, &store),
        )
        .await?;
        Ok(completion.value)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TranscriptionProviderTarget {
    pub config: ProviderConfig,
    pub api_key: String,
}

#[command]
pub async fn ai_provider_status(app: AppHandle) -> Result<Vec<ProviderStatusView>, String> {
    let settings = load_provider_settings(&app)?;
    let store = KeyringSecretStore::new();
    let mut out = Vec::new();

    for cfg in settings.sorted_providers() {
        let provider = match build_provider_from_config(&cfg, &store) {
            Ok(provider) => provider,
            Err(err) => {
                out.push(ProviderStatusView {
                    id: cfg.id,
                    provider: cfg.provider_type,
                    display_name: cfg.label,
                    enabled: cfg.enabled,
                    available: false,
                    priority: cfg.priority,
                    default_model: cfg.default_model,
                    reason: Some(err.to_string()),
                });
                continue;
            }
        };
        let availability = provider.health_check().await;
        out.push(ProviderStatusView {
            id: cfg.id,
            provider: cfg.provider_type,
            display_name: cfg.label,
            enabled: cfg.enabled,
            available: availability.is_ok(),
            priority: cfg.priority,
            default_model: cfg.default_model,
            reason: availability.err().map(|e| e.to_string()),
        });
    }

    Ok(out)
}

#[command]
pub fn ai_has_api_key(provider: AiProviderType) -> Result<KeyStatus, String> {
    let store = KeyringSecretStore::new();
    let has = store
        .get(provider)
        .map_err(|e| e.to_string())?
        .is_some();
    Ok(KeyStatus {
        provider,
        has_key: has,
    })
}

#[command]
pub fn ai_set_api_key(provider: AiProviderType, value: String) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("empty api key".into());
    }
    let store = KeyringSecretStore::new();
    store.set(provider, &value).map_err(|e| e.to_string())
}

#[command]
pub fn ai_delete_api_key(provider: AiProviderType) -> Result<(), String> {
    let store = KeyringSecretStore::new();
    store.delete(provider).map_err(|e| e.to_string())
}

#[command]
pub async fn ai_ping(app: AppHandle, provider: AiProviderType) -> Result<bool, String> {
    let settings = load_provider_settings(&app)?;
    let store = KeyringSecretStore::new();
    for cfg in settings.sorted_providers() {
        if cfg.provider_type != provider {
            continue;
        }
        let instance = build_provider_from_config(&cfg, &store).map_err(|e| e.to_string())?;
        return Ok(instance.health_check().await.is_ok());
    }
    Err("provider not configured".into())
}

#[command]
pub fn ai_get_provider_settings(app: AppHandle) -> Result<ProviderSettingsView, String> {
    let settings = load_provider_settings(&app)?;
    Ok(view_from_settings(&settings))
}

#[command]
pub async fn ai_test_provider(
    input: ProviderTestInput,
) -> Result<ProviderTestResult, String> {
    let capability = input.capability.unwrap_or(ProviderCapability::Text);
    let input = normalize_provider_input(input.provider);
    let config = provider_config_from_input(&input);
    let store = KeyringSecretStore::new();

    if let Some(api_key) = input.api_key.as_deref().filter(|key| !key.trim().is_empty()) {
        store
            .set_by_ref(&config.api_key_ref(), api_key)
            .map_err(|e| e.to_string())?;
    }

    let instance = build_provider_from_config(&config, &store).map_err(|e| e.to_string())?;
    if capability == ProviderCapability::Transcription {
        if !config.supports(ProviderCapability::Transcription) {
            return Ok(ProviderTestResult {
                ok: false,
                message: format!("{} is text-only and cannot be used for audio transcription", config.label),
            });
        }
        if config.provider_type != AiProviderType::OpenAi {
            return Ok(ProviderTestResult {
                ok: false,
                message: format!(
                    "{} is not an OpenAI-compatible audio transcription provider in this version",
                    config.label
                ),
            });
        }
        let api_key = store
            .get_by_ref(&config.api_key_ref())
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("{} API key not set", config.label))?;
        let mut transcriber = transcription_kit::OpenAiWhisperTranscriber::new(api_key);
        if let Some(base_url) = &config.base_url {
            transcriber = transcriber.with_base_url(base_url.clone());
        }
        return match transcriber
            .probe_audio_endpoint(Some(config.default_model.as_str()))
            .await
        {
            Ok(()) => Ok(ProviderTestResult {
                ok: true,
                message: format!("{} audio transcription endpoint is ready", config.label),
            }),
            Err(err) => Ok(ProviderTestResult {
                ok: false,
                message: err,
            }),
        };
    }
    match instance.health_check().await {
        Ok(()) => Ok(ProviderTestResult {
            ok: true,
            message: format!("{} is ready", config.label),
        }),
        Err(err) => Ok(ProviderTestResult {
            ok: false,
            message: err.to_string(),
        }),
    }
}

#[command]
pub fn ai_save_provider_settings(
    app: AppHandle,
    input: ProviderSettingsInput,
) -> Result<ProviderSettingsView, String> {
    let mut providers = input
        .providers
        .into_iter()
        .enumerate()
        .map(|(idx, provider)| {
            let mut provider = normalize_provider_input(provider);
            provider.priority = idx as u32;
            provider
        })
        .collect::<Vec<_>>();

    if providers.is_empty() {
        providers = default_provider_settings()
            .providers
            .into_iter()
            .enumerate()
            .map(|(idx, cfg)| ProviderConfigInput {
                id: Some(cfg.id),
                label: cfg.label,
                provider_type: cfg.provider_type,
                base_url: cfg.base_url,
                default_model: cfg.default_model,
                enabled: cfg.enabled,
                priority: idx as u32,
                capabilities: cfg.capabilities,
                api_key_ref: cfg.api_key_ref,
                api_key: None,
                clear_api_key: false,
            })
            .collect();
    }

    let store = KeyringSecretStore::new();
    let settings = ProviderSettings {
        default_text_provider_id: normalize_default_provider_id(
            input.default_text_provider_id.as_deref(),
            &providers,
            ProviderCapability::Text,
        ),
        default_transcription_provider_id: normalize_default_provider_id(
            input.default_transcription_provider_id.as_deref(),
            &providers,
            ProviderCapability::Transcription,
        ),
        providers: providers
            .iter()
            .map(|provider| {
                let cfg = provider_config_from_input(provider);
                if provider.clear_api_key {
                    let _ = store.delete_by_ref(&cfg.api_key_ref());
                } else if let Some(api_key) = provider.api_key.as_deref() {
                    if !api_key.trim().is_empty() {
                        store
                            .set_by_ref(&cfg.api_key_ref(), api_key)
                            .map_err(|e| e.to_string())?;
                    }
                }
                Ok(cfg)
            })
            .collect::<Result<Vec<_>, String>>()?,
    };

    save_provider_settings(&app, &settings)?;
    Ok(view_from_settings(&settings))
}

pub(crate) fn load_provider_settings(app: &AppHandle) -> Result<ProviderSettings, String> {
    let path = provider_settings_path(app)?;
    if !path.exists() {
        let mut defaults = default_provider_settings();
        merge_workspace_config(app, &mut defaults)?;
        save_provider_settings(app, &defaults)?;
        return Ok(defaults);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read provider settings: {e}"))?;
    let parsed: ProviderSettings = serde_json::from_str(&raw)
        .map_err(|e| format!("cannot parse provider settings: {e}"))?;
    let mut settings = normalize_settings(parsed);
    merge_workspace_config(app, &mut settings)?;
    Ok(settings)
}

pub(crate) fn build_gateway_provider(
    app: &AppHandle,
    tool_name: impl Into<String>,
    capability: ProviderCapability,
    preferred_provider_id: Option<String>,
    preferred_provider: Option<AiProviderType>,
) -> Result<Arc<dyn Provider>, String> {
    let settings = load_provider_settings(app)?;
    let workspace_config = load_workspace_config(app)?;
    let default_provider_id = preferred_provider_id.or_else(|| match capability {
        ProviderCapability::Text => settings.default_text_provider_id.clone(),
        ProviderCapability::Transcription => settings.default_transcription_provider_id.clone(),
        _ => None,
    });
    Ok(Arc::new(ConfiguredGatewayProvider::new(
        settings,
        workspace_config.ai.retry,
        tool_name,
        capability,
        default_provider_id,
        preferred_provider,
    )))
}

pub(crate) async fn resolve_transcription_targets(
    app: &AppHandle,
    preferred_provider_id: Option<&str>,
) -> Result<Vec<TranscriptionProviderTarget>, String> {
    let settings = load_provider_settings(app)?;
    let store = KeyringSecretStore::new();
    let preferred_provider_id = preferred_provider_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| settings.default_transcription_provider_id.clone());

    let mut candidates = settings.sorted_providers();
    candidates.sort_by_key(|cfg| {
        let preferred = preferred_provider_id
            .as_ref()
            .map(|id| (cfg.id != *id) as u8)
            .unwrap_or(0);
        (preferred, cfg.priority)
    });

    let mut targets = Vec::new();
    let mut skipped = Vec::new();
    for cfg in candidates {
        if !cfg.enabled
            || cfg.provider_type != AiProviderType::OpenAi
            || !cfg.supports(ProviderCapability::Transcription)
        {
            continue;
        }
        let Some(api_key) = store
            .get_by_ref(&cfg.api_key_ref())
            .map_err(|e| e.to_string())?
        else {
            skipped.push(format!("{}: API key not set", cfg.label));
            continue;
        };
        targets.push(TranscriptionProviderTarget { config: cfg, api_key });
    }
    if !targets.is_empty() {
        return Ok(targets);
    }

    let legacy_key = store
        .get(AiProviderType::OpenAi)
        .map_err(|e| e.to_string())?;
    if let Some(api_key) = legacy_key {
        return Ok(vec![TranscriptionProviderTarget {
            config: ProviderConfig {
                id: "openai".into(),
                label: "OpenAI".into(),
                provider_type: AiProviderType::OpenAi,
                base_url: Some(OPENAI_API_BASE.into()),
                default_model: "whisper-1".into(),
                enabled: true,
                priority: 0,
                capabilities: vec![ProviderCapability::Text, ProviderCapability::Transcription],
                api_key_ref: Some(KeyringSecretStore::account_for(AiProviderType::OpenAi).into()),
            },
            api_key,
        }]);
    }

    let mut message = "No OpenAI-compatible transcription provider is configured".to_string();
    if !skipped.is_empty() {
        message.push_str(": ");
        message.push_str(&skipped.join(" | "));
    }
    Err(message)
}

fn view_from_settings(settings: &ProviderSettings) -> ProviderSettingsView {
    let store = KeyringSecretStore::new();
    let providers = settings
        .sorted_providers()
        .into_iter()
        .map(|cfg| {
            let has_api_key = match cfg.provider_type {
                AiProviderType::Ollama | AiProviderType::Mlx | AiProviderType::AppleIntelligence => {
                    true
                }
                _ => store
                    .get_by_ref(&cfg.api_key_ref())
                    .map(|value| value.is_some())
                    .unwrap_or(false),
            };
            let api_key_ref = cfg.api_key_ref();
            ProviderConfigView {
                id: cfg.id,
                label: cfg.label,
                provider_type: cfg.provider_type,
                base_url: cfg.base_url,
                default_model: cfg.default_model,
                enabled: cfg.enabled,
                priority: cfg.priority,
                capabilities: cfg.capabilities,
                api_key_ref,
                has_api_key,
            }
        })
        .collect::<Vec<_>>();

    let default_text = resolve_default_provider(
        &providers,
        settings.default_text_provider_id.as_deref(),
        ProviderCapability::Text,
    );
    let default_transcription = resolve_default_provider(
        &providers,
        settings.default_transcription_provider_id.as_deref(),
        ProviderCapability::Transcription,
    );

    ProviderSettingsView {
        providers,
        default_text_provider_id: default_text.as_ref().map(|cfg| cfg.id.clone()),
        default_text_model: default_text.as_ref().map(|cfg| cfg.default_model.clone()),
        default_transcription_provider_id: default_transcription.as_ref().map(|cfg| cfg.id.clone()),
        default_transcription_model: default_transcription
            .as_ref()
            .map(default_transcription_model_for_view),
    }
}

fn resolve_default_provider(
    providers: &[ProviderConfigView],
    preferred_id: Option<&str>,
    capability: ProviderCapability,
) -> Option<ProviderConfigView> {
    preferred_id
        .and_then(|id| {
            providers
                .iter()
                .find(|cfg| {
                    cfg.id == id && provider_is_ready_for_capability(cfg, capability)
                })
                .cloned()
        })
        .or_else(|| {
            providers
                .iter()
                .find(|cfg| provider_is_ready_for_capability(cfg, capability))
                .cloned()
        })
}

fn provider_is_ready_for_capability(
    provider: &ProviderConfigView,
    capability: ProviderCapability,
) -> bool {
    provider.enabled
        && provider.capabilities.contains(&capability)
        && match capability {
            ProviderCapability::Text | ProviderCapability::Transcription => provider.has_api_key,
            _ => true,
        }
}

fn normalize_default_provider_id(
    raw_id: Option<&str>,
    providers: &[ProviderConfigInput],
    capability: ProviderCapability,
) -> Option<String> {
    raw_id
        .map(sanitize_provider_id)
        .filter(|id| {
            providers.iter().any(|provider| {
                provider.enabled
                    && provider.id.as_deref() == Some(id.as_str())
                    && provider.capabilities.contains(&capability)
            })
        })
        .or_else(|| {
            providers
                .iter()
                .find(|provider| provider.enabled && provider.capabilities.contains(&capability))
                .and_then(|provider| provider.id.clone())
        })
}

fn default_provider_settings() -> ProviderSettings {
    ProviderSettings {
        default_text_provider_id: Some("openai".into()),
        default_transcription_provider_id: Some("openai".into()),
        providers: vec![
            ProviderConfig {
                id: "openai".into(),
                label: "OpenAI".into(),
                provider_type: AiProviderType::OpenAi,
                base_url: Some(OPENAI_API_BASE.into()),
                default_model: OPENAI_DEFAULT_MODEL.into(),
                enabled: true,
                priority: 0,
                capabilities: vec![ProviderCapability::Text, ProviderCapability::Transcription],
                api_key_ref: Some(KeyringSecretStore::account_for(AiProviderType::OpenAi).into()),
            },
            ProviderConfig {
                id: "beeknoee".into(),
                label: "Beeknoee".into(),
                provider_type: AiProviderType::OpenAi,
                base_url: Some(BEEKNOEE_API_BASE.into()),
                default_model: BEEKNOEE_DEFAULT_MODEL.into(),
                enabled: false,
                priority: 1,
                capabilities: vec![ProviderCapability::Text],
                api_key_ref: Some("ai.provider.beeknoee.apiKey".into()),
            },
            ProviderConfig {
                id: "openrouter".into(),
                label: "OpenRouter".into(),
                provider_type: AiProviderType::OpenRouter,
                base_url: Some(OPENROUTER_API_BASE.into()),
                default_model: "openai/gpt-4o-mini".into(),
                enabled: false,
                priority: 2,
                capabilities: vec![ProviderCapability::Text],
                api_key_ref: Some(KeyringSecretStore::account_for(AiProviderType::OpenRouter).into()),
            },
            ProviderConfig {
                id: "claude".into(),
                label: "Claude".into(),
                provider_type: AiProviderType::Claude,
                base_url: None,
                default_model: "claude-sonnet-4-5-20250929".into(),
                enabled: false,
                priority: 3,
                capabilities: vec![ProviderCapability::Text],
                api_key_ref: Some(KeyringSecretStore::account_for(AiProviderType::Claude).into()),
            },
            ProviderConfig {
                id: "gemini".into(),
                label: "Gemini".into(),
                provider_type: AiProviderType::Gemini,
                base_url: None,
                default_model: "gemini-2.0-flash".into(),
                enabled: false,
                priority: 4,
                capabilities: vec![ProviderCapability::Text],
                api_key_ref: Some(KeyringSecretStore::account_for(AiProviderType::Gemini).into()),
            },
            ProviderConfig {
                id: "ollama".into(),
                label: "Ollama".into(),
                provider_type: AiProviderType::Ollama,
                base_url: Some(DEFAULT_HOST.into()),
                default_model: "llama3.2".into(),
                enabled: false,
                priority: 5,
                capabilities: vec![ProviderCapability::Text],
                api_key_ref: Some(KeyringSecretStore::account_for(AiProviderType::Ollama).into()),
            },
        ],
    }
}

fn load_workspace_config(_app: &AppHandle) -> Result<WorkspaceConfigFile, String> {
    let path = workspace_config_path();
    if !path.exists() {
        return Ok(WorkspaceConfigFile::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("cannot parse {}: {e}", path.display()))
}

fn merge_workspace_config(app: &AppHandle, settings: &mut ProviderSettings) -> Result<(), String> {
    let workspace_config = load_workspace_config(app)?;
    if let Some(default_text_provider_id) = workspace_config.ai.default_text_provider_id {
        settings.default_text_provider_id = Some(sanitize_provider_id(&default_text_provider_id));
    }
    if let Some(default_transcription_provider_id) =
        workspace_config.ai.default_transcription_provider_id
    {
        settings.default_transcription_provider_id =
            Some(sanitize_provider_id(&default_transcription_provider_id));
    }
    if workspace_config.ai.providers.is_empty() {
        return Ok(());
    }
    let store = KeyringSecretStore::new();
    for provider in workspace_config.ai.providers {
        let config = ProviderConfig {
            id: sanitize_provider_id(&provider.id),
            label: provider.label.trim().to_string(),
            provider_type: provider.provider_type,
            base_url: sanitize_optional(provider.base_url),
            default_model: provider
                .default_model
                .unwrap_or_else(|| default_model_for(provider.provider_type).to_string()),
            enabled: provider.enabled.unwrap_or(true),
            priority: provider.priority.unwrap_or(0),
            capabilities: if provider.capabilities.is_empty() {
                vec![ProviderCapability::Text]
            } else {
                provider.capabilities
            },
            api_key_ref: Some(
                provider
                    .api_key_ref
                    .unwrap_or_else(|| format!("ai.provider.{}.apiKey", sanitize_provider_id(&provider.id))),
            ),
        };
        if let Some(api_key) = provider.api_key.filter(|value| !value.trim().is_empty()) {
            store
                .set_by_ref(&config.api_key_ref(), &api_key)
                .map_err(|e| e.to_string())?;
        }
        if let Some(existing) = settings.providers.iter_mut().find(|item| item.id == config.id) {
            *existing = config;
        } else {
            settings.providers.push(config);
        }
    }
    settings.providers.sort_by_key(|cfg| cfg.priority);
    Ok(())
}

fn normalize_settings(settings: ProviderSettings) -> ProviderSettings {
    let default_text_provider_id = settings
        .default_text_provider_id
        .as_deref()
        .map(sanitize_provider_id)
        .filter(|value| !value.is_empty());
    let default_transcription_provider_id = settings
        .default_transcription_provider_id
        .as_deref()
        .map(sanitize_provider_id)
        .filter(|value| !value.is_empty());
    let mut providers = settings
        .providers
        .into_iter()
        .enumerate()
        .map(|(idx, provider)| {
            let mut input = ProviderConfigInput {
                id: Some(provider.id),
                label: provider.label,
                provider_type: provider.provider_type,
                base_url: provider.base_url,
                default_model: provider.default_model,
                enabled: provider.enabled,
                priority: idx as u32,
                capabilities: provider.capabilities,
                api_key_ref: provider.api_key_ref,
                api_key: None,
                clear_api_key: false,
            };
            input = normalize_provider_input(input);
            provider_config_from_input(&input)
        })
        .collect::<Vec<_>>();
    for default_provider in default_provider_settings().providers {
        if providers.iter().any(|cfg| cfg.id == default_provider.id) {
            continue;
        }
        providers.push(default_provider);
    }
    providers.sort_by_key(|cfg| cfg.priority);
    ProviderSettings {
        default_text_provider_id,
        default_transcription_provider_id,
        providers,
    }
}

fn normalize_provider_input(mut input: ProviderConfigInput) -> ProviderConfigInput {
    let generated_id = input
        .id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .as_deref()
        .map(sanitize_provider_id)
        .unwrap_or_else(|| sanitize_provider_id(&format!("{}-{}", input.label, Uuid::new_v4())));
    input.id = Some(generated_id);
    input.label = input.label.trim().to_string();
    if input.label.is_empty() {
        input.label = input.provider_type.display_name().to_string();
    }
    input.default_model = input.default_model.trim().to_string();
    if input.default_model.is_empty() {
        input.default_model = default_model_for(input.provider_type).into();
    }
    input.base_url = sanitize_optional(input.base_url);
    if input.capabilities.is_empty() {
        input.capabilities = vec![ProviderCapability::Text];
    }
    input
}

fn provider_config_from_input(input: &ProviderConfigInput) -> ProviderConfig {
    let id = input.id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
    ProviderConfig {
        id: id.clone(),
        label: input.label.clone(),
        provider_type: input.provider_type,
        base_url: input.base_url.clone().or_else(|| default_base_url_for(input.provider_type)),
        default_model: input.default_model.clone(),
        enabled: input.enabled,
        priority: input.priority,
        capabilities: input.capabilities.clone(),
        api_key_ref: Some(
            input
                .api_key_ref
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| format!("ai.provider.{id}.apiKey")),
        ),
    }
}

fn build_provider_from_config(
    config: &ProviderConfig,
    store: &KeyringSecretStore,
) -> Result<Arc<dyn Provider>, AiProviderError> {
    match config.provider_type {
        AiProviderType::OpenAi => {
            let key = store
                .get_by_ref(&config.api_key_ref())?
                .ok_or(AiProviderError::MissingApiKey(AiProviderType::OpenAi))?;
            let mut provider = OpenAiProvider::new(key);
            if let Some(base_url) = &config.base_url {
                provider = provider.with_base_url(base_url.clone());
            }
            Ok(Arc::new(provider))
        }
        AiProviderType::OpenRouter => {
            let key = store
                .get_by_ref(&config.api_key_ref())?
                .ok_or(AiProviderError::MissingApiKey(AiProviderType::OpenRouter))?;
            let mut provider = OpenRouterProvider::new(key);
            if let Some(base_url) = &config.base_url {
                provider = provider.with_base_url(base_url.clone());
            }
            Ok(Arc::new(provider))
        }
        AiProviderType::Claude => {
            let key = store
                .get_by_ref(&config.api_key_ref())?
                .ok_or(AiProviderError::MissingApiKey(AiProviderType::Claude))?;
            Ok(Arc::new(ClaudeProvider::new(key)))
        }
        AiProviderType::Gemini => {
            let key = store
                .get_by_ref(&config.api_key_ref())?
                .ok_or(AiProviderError::MissingApiKey(AiProviderType::Gemini))?;
            Ok(Arc::new(GeminiProvider::new(key)))
        }
        AiProviderType::Ollama => {
            let host = config
                .base_url
                .clone()
                .or_else(|| store.get_by_ref(&config.api_key_ref()).ok().flatten())
                .unwrap_or_else(|| DEFAULT_HOST.to_string());
            Ok(Arc::new(OllamaProvider::new(host)))
        }
        AiProviderType::Mlx => {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            {
                return Ok(Arc::new(ai_kit::MlxLmProvider::default_local()));
            }
            #[allow(unreachable_code)]
            Err(AiProviderError::NotAvailable(AiProviderType::Mlx))
        }
        AiProviderType::AppleIntelligence => Err(AiProviderError::NotAvailable(
            AiProviderType::AppleIntelligence,
        )),
    }
}

fn provider_settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("cannot resolve app config dir: {e}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create app config dir {}: {e}", dir.display()))?;
    Ok(dir.join(PROVIDER_SETTINGS_FILE))
}

fn workspace_config_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(WORKSPACE_CONFIG_FILE)
}

fn save_provider_settings(app: &AppHandle, settings: &ProviderSettings) -> Result<(), String> {
    let path = provider_settings_path(app)?;
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("cannot serialize provider settings: {e}"))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("cannot write provider settings {}: {e}", path.display()))
}

fn sanitize_provider_id(raw: &str) -> String {
    let mut out = raw
        .chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' => ch,
            'A'..='Z' => ch.to_ascii_lowercase(),
            _ => '-',
        })
        .collect::<String>();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}

fn sanitize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn default_model_for(provider_type: AiProviderType) -> &'static str {
    match provider_type {
        AiProviderType::OpenAi => OPENAI_DEFAULT_MODEL,
        AiProviderType::OpenRouter => "openai/gpt-4o-mini",
        AiProviderType::Claude => "claude-sonnet-4-5-20250929",
        AiProviderType::Gemini => "gemini-2.0-flash",
        AiProviderType::Ollama => "llama3.2",
        AiProviderType::Mlx => "mlx-community/Qwen3-14B-4bit",
        AiProviderType::AppleIntelligence => "apple-intelligence-default",
    }
}

fn default_base_url_for(provider_type: AiProviderType) -> Option<String> {
    match provider_type {
        AiProviderType::OpenAi => Some(OPENAI_API_BASE.into()),
        AiProviderType::OpenRouter => Some(OPENROUTER_API_BASE.into()),
        AiProviderType::Ollama => Some(DEFAULT_HOST.into()),
        _ => None,
    }
}

fn default_transcription_model_for_view(provider: &ProviderConfigView) -> String {
    match provider.provider_type {
        AiProviderType::OpenAi => "whisper-1".into(),
        _ => provider.default_model.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_is_sanitized() {
        assert_eq!(sanitize_provider_id("My Custom Provider!"), "my-custom-provider");
    }

    #[test]
    fn normalization_fills_missing_defaults() {
        let input = normalize_provider_input(ProviderConfigInput {
            id: None,
            label: "OpenAI Mirror".into(),
            provider_type: AiProviderType::OpenAi,
            base_url: Some(" https://example.com ".into()),
            default_model: "".into(),
            enabled: true,
            priority: 0,
            capabilities: vec![],
            api_key_ref: None,
            api_key: None,
            clear_api_key: false,
        });
        assert!(input.id.unwrap().starts_with("openai-mirror-"));
        assert_eq!(input.base_url.as_deref(), Some("https://example.com"));
        assert_eq!(input.default_model, OPENAI_DEFAULT_MODEL);
        assert_eq!(input.capabilities, vec![ProviderCapability::Text]);
    }

    #[test]
    fn normalization_backfills_beeknoee_default_provider() {
        let settings = ProviderSettings {
            default_text_provider_id: None,
            default_transcription_provider_id: None,
            providers: vec![ProviderConfig {
                id: "openai".into(),
                label: "OpenAI".into(),
                provider_type: AiProviderType::OpenAi,
                base_url: Some(OPENAI_API_BASE.into()),
                default_model: OPENAI_DEFAULT_MODEL.into(),
                enabled: true,
                priority: 0,
                capabilities: vec![ProviderCapability::Text],
                api_key_ref: None,
            }],
        };
        let normalized = normalize_settings(settings);
        assert!(normalized.providers.iter().any(|cfg| cfg.id == "beeknoee"));
    }
}
