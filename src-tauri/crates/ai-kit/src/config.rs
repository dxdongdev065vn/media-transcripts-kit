use serde::{Deserialize, Serialize};

use creator_core::AiProviderType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderCapability {
    Text,
    Image,
    Vision,
    Transcription,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub label: String,
    pub provider_type: AiProviderType,
    #[serde(default)]
    pub base_url: Option<String>,
    pub default_model: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: u32,
    #[serde(default = "default_capabilities")]
    pub capabilities: Vec<ProviderCapability>,
    #[serde(default)]
    pub api_key_ref: Option<String>,
}

fn default_enabled() -> bool {
    true
}

fn default_capabilities() -> Vec<ProviderCapability> {
    vec![ProviderCapability::Text]
}

impl ProviderConfig {
    pub fn supports(&self, capability: ProviderCapability) -> bool {
        self.capabilities.contains(&capability)
    }

    pub fn api_key_ref(&self) -> String {
        self.api_key_ref
            .clone()
            .unwrap_or_else(|| format!("ai.provider.{}.apiKey", self.id))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    #[serde(default)]
    pub default_text_provider_id: Option<String>,
    #[serde(default)]
    pub default_transcription_provider_id: Option<String>,
    pub providers: Vec<ProviderConfig>,
}

impl ProviderSettings {
    pub fn sorted_providers(&self) -> Vec<ProviderConfig> {
        let mut providers = self.providers.clone();
        providers.sort_by_key(|cfg| cfg.priority);
        providers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_ref_defaults_to_provider_id() {
        let cfg = ProviderConfig {
            id: "custom-openai".into(),
            label: "Custom".into(),
            provider_type: AiProviderType::OpenAi,
            base_url: Some("https://example.com".into()),
            default_model: "gpt-4o-mini".into(),
            enabled: true,
            priority: 0,
            capabilities: vec![ProviderCapability::Text],
            api_key_ref: None,
        };
        assert_eq!(cfg.api_key_ref(), "ai.provider.custom-openai.apiKey");
    }

    #[test]
    fn supports_checks_capabilities() {
        let cfg = ProviderConfig {
            id: "x".into(),
            label: "X".into(),
            provider_type: AiProviderType::OpenAi,
            base_url: None,
            default_model: "m".into(),
            enabled: true,
            priority: 0,
            capabilities: vec![ProviderCapability::Text, ProviderCapability::Transcription],
            api_key_ref: None,
        };
        assert!(cfg.supports(ProviderCapability::Text));
        assert!(!cfg.supports(ProviderCapability::Image));
    }
}
