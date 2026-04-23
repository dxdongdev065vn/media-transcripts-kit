//! Shared app state for Tauri commands.
//!
//! Two caches:
//!
//!   `pcm`        — Arc<Vec<f32>> of 16 kHz mono samples. Extracted once,
//!                  reused by silence detection, transcription, and any
//!                  future DSP features. Saves repeated ffmpeg invocations
//!                  when the user tweaks sliders or reruns transcription.
//!   `transcripts`— Arc<TranscriptEntry> with the whisper output for a
//!                  clip, keyed by source + backend/provider/model so
//!                  switching providers does not silently reuse stale ASR.
//!
//! Both caches are `Arc`-shared so cloning is cheap; a command can hold
//! the Arc while the cache itself is swapped by another command. Eviction
//! is manual via `clear` — memory footprint is bounded by how many clips
//! the user touches in one session, which is small in practice.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use creator_core::TranscriptionSegment;

/// Snapshot of a whisper run for one clip. Stored in the transcript cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub backend: String,
    pub provider_id: Option<String>,
    pub model: String,
    pub language: Option<String>,
    pub segments: Vec<TranscriptionSegment>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct TranscriptCacheKey {
    pub path: PathBuf,
    pub backend: String,
    pub provider_id: Option<String>,
    pub model: String,
}

impl TranscriptCacheKey {
    pub fn new(
        path: PathBuf,
        backend: impl Into<String>,
        provider_id: Option<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            path,
            backend: backend.into(),
            provider_id: provider_id
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            model: model.into(),
        }
    }
}

pub struct AppState {
    pub pcm: Mutex<HashMap<PathBuf, Arc<Vec<f32>>>>,
    pub transcripts: Mutex<HashMap<TranscriptCacheKey, Arc<TranscriptEntry>>>,
    /// PID of the mlx_lm.server process spawned by this app, if any.
    /// Killed when the app exits.
    pub mlx_server_pid: Mutex<Option<u32>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            pcm: Mutex::new(HashMap::new()),
            transcripts: Mutex::new(HashMap::new()),
            mlx_server_pid: Mutex::new(None),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the cached PCM buffer for `path`, if any.
    pub fn pcm_get(&self, path: &Path) -> Option<Arc<Vec<f32>>> {
        self.pcm.lock().ok()?.get(path).cloned()
    }

    pub fn pcm_put(&self, path: PathBuf, samples: Vec<f32>) -> Arc<Vec<f32>> {
        let arc = Arc::new(samples);
        if let Ok(mut map) = self.pcm.lock() {
            map.insert(path, arc.clone());
        }
        arc
    }

    pub fn transcript_get(&self, key: &TranscriptCacheKey) -> Option<Arc<TranscriptEntry>> {
        self.transcripts.lock().ok()?.get(key).cloned()
    }

    pub fn transcript_get_any_for_path(&self, path: &Path) -> Option<Arc<TranscriptEntry>> {
        self.transcripts
            .lock()
            .ok()?
            .iter()
            .find(|(key, _)| key.path == path)
            .map(|(_, value)| value.clone())
    }

    pub fn transcript_put(&self, key: TranscriptCacheKey, entry: TranscriptEntry) -> Arc<TranscriptEntry> {
        let arc = Arc::new(entry);
        if let Ok(mut map) = self.transcripts.lock() {
            map.insert(key, arc.clone());
        }
        arc
    }

    pub fn clear_for(&self, path: &Path) {
        if let Ok(mut map) = self.pcm.lock() {
            map.remove(path);
        }
        if let Ok(mut map) = self.transcripts.lock() {
            map.retain(|key, _| key.path != path);
        }
    }

    pub fn clear_all(&self) {
        if let Ok(mut map) = self.pcm.lock() {
            map.clear();
        }
        if let Ok(mut map) = self.transcripts.lock() {
            map.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_round_trip() {
        let s = AppState::new();
        let p = PathBuf::from("/tmp/video.mov");
        assert!(s.pcm_get(&p).is_none());
        s.pcm_put(p.clone(), vec![0.1, 0.2, 0.3]);
        let got = s.pcm_get(&p).unwrap();
        assert_eq!(got.as_slice(), &[0.1, 0.2, 0.3]);
    }

    #[test]
    fn transcript_round_trip() {
        let s = AppState::new();
        let p = PathBuf::from("/tmp/video.mov");
        let entry = TranscriptEntry {
            backend: "openai".into(),
            provider_id: Some("openai".into()),
            model: "whisper-1".into(),
            language: Some("en".into()),
            segments: vec![TranscriptionSegment::new(0, 1_000, "hello")],
        };
        let key = TranscriptCacheKey::new(p.clone(), "openai", Some("openai".into()), "whisper-1");
        s.transcript_put(key.clone(), entry);
        let got = s.transcript_get(&key).unwrap();
        assert_eq!(got.segments.len(), 1);
        assert_eq!(got.language.as_deref(), Some("en"));
    }

    #[test]
    fn clear_for_drops_both_caches() {
        let s = AppState::new();
        let p = PathBuf::from("/tmp/x.mov");
        s.pcm_put(p.clone(), vec![0.0]);
        let key = TranscriptCacheKey::new(p.clone(), "mlx", Some("mlx".into()), "mlx-community/whisper-large-v3-turbo");
        s.transcript_put(
            key,
            TranscriptEntry {
                backend: "mlx".into(),
                provider_id: Some("mlx".into()),
                model: "mlx-community/whisper-large-v3-turbo".into(),
                language: None,
                segments: vec![],
            },
        );
        s.clear_for(&p);
        assert!(s.pcm_get(&p).is_none());
        assert!(s.transcripts.lock().unwrap().is_empty());
    }
}
