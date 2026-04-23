//! Import an existing `.srt` subtitle file as the session transcript.
//!
//! Lets translate-only users skip transcription entirely: drop a `.srt`,
//! the parsed segments are seeded into `AppState.transcripts` exactly like
//! a whisper result, and every downstream feature (translate, summary,
//! chapters, YT pack, viral clips) works unchanged.

use std::path::PathBuf;

use tauri::{command, State};

use crate::commands::output::parse_srt;
use crate::commands::transcription::TranscribeOutput;
use crate::state::{AppState, TranscriptCacheKey, TranscriptEntry};

#[command]
pub async fn import_srt(
    path: String,
    state: State<'_, AppState>,
) -> Result<TranscribeOutput, String> {
    let source = PathBuf::from(&path);
    let content = std::fs::read_to_string(&source)
        .map_err(|e| format!("read {}: {e}", source.display()))?;
    // Strip UTF-8 BOM if present.
    let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
    let segments = parse_srt(content);
    if segments.is_empty() {
        return Err("no segments found in SRT file".into());
    }
    let entry = TranscriptEntry {
        backend: "import-srt".into(),
        provider_id: None,
        model: "srt".into(),
        language: None,
        segments,
    };
    let cache_key = TranscriptCacheKey::new(source, "import-srt", None, "srt");
    let arc = state.transcript_put(cache_key, entry);
    Ok(TranscribeOutput {
        language: arc.language.clone(),
        segments: arc.segments.clone(),
        from_cache: false,
    })
}
