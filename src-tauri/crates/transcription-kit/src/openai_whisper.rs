//! OpenAI Whisper API transcriber.
//!
//! Sends the audio file to `/v1/audio/transcriptions` with `verbose_json`
//! response format so we get segment-level timestamps. Works on any platform
//! that has an OpenAI API key — the intended fallback for non-Apple-Silicon
//! machines where MLX Whisper is unavailable.
//!
//! Size limit: OpenAI caps uploads at 25 MB. The caller should pre-extract
//! a 32 kbps mono MP3 via `media_kit::extract_audio_mp3` (≈14 MB / hour)
//! before invoking this transcriber.

use creator_core::TranscriptionSegment;
use reqwest::multipart;
use serde::Deserialize;
use std::path::Path;

const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

pub struct OpenAiWhisperTranscriber {
    pub api_key: String,
    pub base_url: String,
}

// ── OpenAI verbose_json schema ──────────────────────────────────────────────

#[derive(Deserialize)]
struct VerboseResponse {
    language: Option<String>,
    segments: Vec<OaSegment>,
}

#[derive(Deserialize)]
struct OaSegment {
    start: f64,
    end: f64,
    text: String,
}

// ── impl ────────────────────────────────────────────────────────────────────

impl OpenAiWhisperTranscriber {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com".into(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    fn normalized_base_url(&self) -> String {
        normalize_openai_audio_base_url(&self.base_url)
    }

    pub async fn probe_audio_endpoint(&self, model: Option<&str>) -> Result<(), String> {
        let model = model.unwrap_or("whisper-1").to_string();
        let part = multipart::Part::bytes(vec![0_u8; 4])
            .file_name("probe.mp3")
            .mime_str("audio/mpeg")
            .map_err(|e| e.to_string())?;
        let form = multipart::Form::new()
            .part("file", part)
            .text("model", model)
            .text("response_format", "verbose_json");

        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "{}/audio/transcriptions",
                self.normalized_base_url()
            ))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("Audio transcription probe failed: {e}"))?;

        if resp.status().is_success() {
            return Ok(());
        }

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if is_unsupported_audio_endpoint(status, &body) {
            return Err(unsupported_audio_endpoint_message());
        }
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(format!("Audio transcription probe failed {status}: invalid API key or permission denied"));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            return Err(format!("Audio transcription probe failed {status}: provider temporarily unavailable"));
        }
        Ok(())
    }

    /// Transcribe `audio_path` and return timestamped segments.
    ///
    /// `model` defaults to `"whisper-1"`.
    /// `language` is an optional BCP-47 hint (e.g. `"en"`, `"vi"`).
    pub async fn transcribe(
        &self,
        audio_path: &Path,
        language: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<TranscriptionSegment>, String> {
        let model = model.unwrap_or("whisper-1").to_string();

        let file_bytes = tokio::fs::read(audio_path)
            .await
            .map_err(|e| format!("cannot read audio file: {e}"))?;
        if file_bytes.len() > MAX_UPLOAD_BYTES {
            let size_mb = file_bytes.len() as f64 / (1024.0 * 1024.0);
            return Err(format!(
                "Audio is too large for cloud transcription ({size_mb:.1} MB after extraction, limit is 25 MB). Use a shorter clip or local transcription."
            ));
        }

        let filename = audio_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.mp3")
            .to_string();

        let part = multipart::Part::bytes(file_bytes)
            .file_name(filename)
            .mime_str("audio/mpeg")
            .map_err(|e| e.to_string())?;

        let mut form = multipart::Form::new()
            .part("file", part)
            .text("model", model)
            .text("response_format", "verbose_json");

        if let Some(lang) = language {
            form = form.text("language", lang.to_string());
        }

        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "{}/audio/transcriptions",
                self.normalized_base_url()
            ))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("OpenAI request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if is_unsupported_audio_endpoint(status, &body) {
                return Err(unsupported_audio_endpoint_message());
            }
            return Err(format!("OpenAI Whisper error {status}: {body}"));
        }

        let data: VerboseResponse = resp
            .json()
            .await
            .map_err(|e| format!("failed to parse OpenAI response: {e}"))?;

        let lang_tag = data.language.clone();
        let segments = data
            .segments
            .into_iter()
            .map(|s| {
                let mut seg = TranscriptionSegment::new(
                    (s.start * 1000.0) as i64,
                    (s.end * 1000.0) as i64,
                    s.text.trim(),
                );
                seg.language = lang_tag.clone();
                seg
            })
            .collect();

        Ok(segments)
    }
}

fn normalize_openai_audio_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.ends_with("/audio/transcriptions") {
        return trimmed
            .trim_end_matches("/audio/transcriptions")
            .trim_end_matches('/')
            .to_string();
    }
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

fn looks_like_html_not_found(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("<!doctype html")
        || lower.contains("<html")
        || lower.contains("this page could not be found")
        || lower.contains("<title>404")
}

pub fn unsupported_audio_endpoint_message() -> String {
    "Selected provider does not expose an OpenAI-compatible audio transcription endpoint. Use OpenAI for cloud transcription, or keep this provider for text-only tasks.".into()
}

pub fn is_unsupported_audio_endpoint(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::NOT_FOUND && looks_like_html_not_found(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_full_chat_endpoint_to_v1_base() {
        assert_eq!(
            normalize_openai_audio_base_url("https://platform.beeknoee.com/api/v1/chat/completions"),
            "https://platform.beeknoee.com/api/v1"
        );
    }

    #[test]
    fn preserves_v1_base() {
        assert_eq!(
            normalize_openai_audio_base_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn detects_html_404_page() {
        assert!(looks_like_html_not_found("<!DOCTYPE html><html><title>404</title></html>"));
    }
}
