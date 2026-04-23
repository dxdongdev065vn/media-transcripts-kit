//! Transcription + cache commands.
//!
//! Apple Silicon uses the `mlx_whisper` sidecar; cloud mode uses the first
//! configured OpenAI-compatible transcription provider from provider settings.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{command, AppHandle, State};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use tauri::Emitter;

use creator_core::TranscriptionSegment;

use crate::state::{AppState, TranscriptCacheKey, TranscriptEntry};

struct TempAudio(PathBuf);
impl Drop for TempAudio {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn prepare_audio(source: &Path) -> Result<(PathBuf, TempAudio), String> {
    use uuid::Uuid;
    let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("audio");
    let tmp = std::env::temp_dir().join(format!("{stem}_asr_{}.mp3", Uuid::new_v4()));
    media_kit::extract_audio_mp3(source, &tmp)
        .await
        .map_err(|e| format!("audio extraction failed: {e}"))?;
    let guard = TempAudio(tmp.clone());
    Ok((tmp, guard))
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const PROGRESS_EVENT: &str = "mlx_whisper_progress";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[derive(Debug, Clone, Serialize)]
pub struct ProgressPayload {
    pub current_ms: i64,
    pub total_ms: i64,
    pub percent: f32,
}

#[derive(Debug, Serialize)]
pub struct TranscribeOutput {
    pub language: Option<String>,
    pub segments: Vec<TranscriptionSegment>,
    pub from_cache: bool,
}

impl TranscribeOutput {
    fn from_entry(entry: Arc<TranscriptEntry>, from_cache: bool) -> Self {
        Self {
            language: entry.language.clone(),
            segments: entry.segments.clone(),
            from_cache,
        }
    }
}

fn transcript_cache_key(
    path: &Path,
    backend: &str,
    provider_id: Option<&str>,
    model: &str,
) -> TranscriptCacheKey {
    TranscriptCacheKey::new(
        path.to_path_buf(),
        backend.to_string(),
        provider_id.map(|value| value.to_string()),
        model.to_string(),
    )
}

fn should_try_next_transcription_provider(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("does not expose an openai-compatible audio transcription endpoint")
        || lower.contains("temporarily unavailable")
        || lower.contains("invalid api key")
        || lower.contains("permission denied")
        || lower.contains("429")
        || lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("rate limit")
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[command]
pub async fn mlx_whisper_transcribe(
    path: String,
    language: Option<String>,
    model: Option<String>,
    force: Option<bool>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TranscribeOutput, String> {
    use std::sync::Arc as StdArc;
    use transcription_kit::{MlxWhisperTranscriber, TranscriptionOptions};

    let source = PathBuf::from(&path);
    let refresh = force.unwrap_or(false);
    let model_id = model
        .clone()
        .unwrap_or_else(|| "mlx-community/whisper-large-v3-turbo".into());
    let cache_key = transcript_cache_key(&source, "mlx", Some("mlx"), &model_id);

    if !refresh {
        if let Some(hit) = state.transcript_get(&cache_key) {
            return Ok(TranscribeOutput::from_entry(hit, true));
        }
    }

    let (audio_path, _audio_guard) = prepare_audio(&source).await?;
    let total_ms = media_kit::probe_media(&source)
        .await
        .map(|p| p.duration_ms)
        .unwrap_or(0);

    let mut transcriber = MlxWhisperTranscriber::new();
    if let Some(m) = model {
        transcriber = transcriber.with_model(m);
    }
    let mut options = TranscriptionOptions::default();
    options.language = language;

    let _ = app.emit(
        PROGRESS_EVENT,
        ProgressPayload {
            current_ms: 0,
            total_ms,
            percent: 0.0,
        },
    );

    let app_for_cb = app.clone();
    let on_progress: transcription_kit::ProgressCallback = StdArc::new(move |end_ms: i64| {
        let percent = if total_ms > 0 {
            ((end_ms as f32 / total_ms as f32) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let _ = app_for_cb.emit(
            PROGRESS_EVENT,
            ProgressPayload {
                current_ms: end_ms,
                total_ms,
                percent,
            },
        );
    });

    let segments = transcriber
        .transcribe_file_with_progress(&audio_path, &options, on_progress)
        .await?;

    let _ = app.emit(
        PROGRESS_EVENT,
        ProgressPayload {
            current_ms: total_ms,
            total_ms,
            percent: 100.0,
        },
    );

    let language = segments.iter().find_map(|s| s.language.clone());
    let entry = TranscriptEntry {
        backend: "mlx".into(),
        provider_id: Some("mlx".into()),
        model: model_id,
        language,
        segments,
    };
    let arc = state.transcript_put(cache_key, entry);
    Ok(TranscribeOutput::from_entry(arc, false))
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
#[command]
pub async fn mlx_whisper_transcribe(
    _path: String,
    _language: Option<String>,
    _model: Option<String>,
    _force: Option<bool>,
    _app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<TranscribeOutput, String> {
    Err("mlx_whisper backend is only available on Apple Silicon (macOS aarch64)".into())
}

#[command]
pub async fn openai_whisper_transcribe(
    app: AppHandle,
    path: String,
    provider_id: Option<String>,
    language: Option<String>,
    model: Option<String>,
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<TranscribeOutput, String> {
    use transcription_kit::OpenAiWhisperTranscriber;

    let source = PathBuf::from(&path);
    let refresh = force.unwrap_or(false);
    let (audio_path, _audio_guard) = prepare_audio(&source).await?;
    let targets = crate::commands::ai::resolve_transcription_targets(&app, provider_id.as_deref()).await?;
    let mut failures = Vec::new();

    for target in targets {
        let model_id = model
            .clone()
            .unwrap_or_else(|| target.config.default_model.clone());
        let cache_key = transcript_cache_key(
            &source,
            "openai-compatible-audio",
            Some(target.config.id.as_str()),
            &model_id,
        );
        if !refresh {
            if let Some(hit) = state.transcript_get(&cache_key) {
                return Ok(TranscribeOutput::from_entry(hit, true));
            }
        }

        let mut transcriber = OpenAiWhisperTranscriber::new(target.api_key.clone());
        if let Some(base_url) = target.config.base_url.clone() {
            transcriber = transcriber.with_base_url(base_url);
        }

        if let Err(err) = transcriber
            .probe_audio_endpoint(Some(model_id.as_str()))
            .await
        {
            failures.push(format!("{}: {}", target.config.label, err));
            if should_try_next_transcription_provider(&err) {
                continue;
            }
            return Err(err);
        }

        match transcriber
            .transcribe(&audio_path, language.as_deref(), Some(model_id.as_str()))
            .await
        {
            Ok(segments) => {
                let language = segments.iter().find_map(|s| s.language.clone());
                let entry = TranscriptEntry {
                    backend: "openai-compatible-audio".into(),
                    provider_id: Some(target.config.id.clone()),
                    model: model_id,
                    language,
                    segments,
                };
                let arc = state.transcript_put(cache_key, entry);
                return Ok(TranscribeOutput::from_entry(arc, false));
            }
            Err(err) => {
                failures.push(format!("{}: {}", target.config.label, err));
                if should_try_next_transcription_provider(&err) {
                    continue;
                }
                return Err(err);
            }
        }
    }

    Err(format!(
        "No working cloud transcription provider is available: {}",
        failures.join(" | ")
    ))
}

#[command]
pub async fn get_cached_transcript(
    path: String,
    backend: Option<String>,
    provider_id: Option<String>,
    model: Option<String>,
    state: State<'_, AppState>,
) -> Result<Option<TranscribeOutput>, String> {
    let source = PathBuf::from(&path);
    if backend.is_none() && provider_id.is_none() && model.is_none() {
        return Ok(state
            .transcript_get_any_for_path(&source)
            .map(|arc| TranscribeOutput::from_entry(arc, true)));
    }
    let backend = backend.unwrap_or_else(|| "openai-compatible-audio".into());
    let model = model.unwrap_or_else(|| "whisper-1".into());
    let key = transcript_cache_key(&source, &backend, provider_id.as_deref(), &model);
    Ok(state
        .transcript_get(&key)
        .map(|arc| TranscribeOutput::from_entry(arc, true)))
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub is_apple_silicon: bool,
    pub mlx_runtime_available: bool,
}

#[command]
pub async fn check_platform() -> PlatformInfo {
    let is_silicon = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    PlatformInfo {
        is_apple_silicon: is_silicon,
        mlx_runtime_available: is_silicon && mlx_runtime_installed(),
    }
}

fn mlx_runtime_installed() -> bool {
    fn find(name: &str) -> bool {
        if std::process::Command::new("which")
            .arg(name)
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false)
        {
            return true;
        }
        let home = std::env::var("HOME").unwrap_or_default();
        let candidates = [
            format!("{home}/.local/bin/{name}"),
            format!("/opt/homebrew/bin/{name}"),
            format!("/usr/local/bin/{name}"),
        ];
        candidates.iter().any(|c| std::path::Path::new(c).exists())
    }
    find("mlx_whisper") && find("mlx_lm.server")
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[command]
pub async fn mlx_model_ready() -> Result<bool, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let snapshots = std::path::Path::new(&home)
        .join(".cache/huggingface/hub/models--mlx-community--whisper-large-v3-turbo/snapshots");
    let ready = snapshots.exists()
        && std::fs::read_dir(&snapshots)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);
    Ok(ready)
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
#[command]
pub async fn mlx_model_ready() -> Result<bool, String> {
    Ok(false)
}

#[command]
pub async fn clear_cache(
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    match path {
        Some(p) => state.clear_for(&PathBuf::from(p)),
        None => state.clear_all(),
    }
    Ok(())
}
