# Architecture

Tauri 2 desktop app — Rust backend + vanilla JS/HTML frontend.

## Stack

- **Frontend**: plain ES modules, no framework. Entry `src/index.html` + `src/js/main.js`.
- **Backend**: Rust workspace under `src-tauri/`. Tauri commands bridge JS ↔ Rust via IPC.
- **Bundled sidecars**: `ffmpeg`, `ffprobe`, `yt-dlp` — downloaded per-platform during CI build, shipped inside the app bundle. Path resolution in [`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs).
- **AI runtime**:
  - **Cloud**: OpenAI / Claude / Gemini / OpenRouter / Ollama — HTTP providers in [`ai-kit`](../src-tauri/crates/ai-kit/).
  - **Local (Apple Silicon only)**: `mlx_whisper` CLI for ASR, `mlx_lm.server` for LLM. Both are Python sidecars launched as subprocesses; user installs via pip/pipx.

## Crate graph

```
my-media-kit (app)            ← Tauri shell + commands + state
├── creator-core               ← shared types: TranscriptionSegment, AiProviderType
├── ai-kit                     ← Provider trait + HTTP clients (OpenAI, Claude, …, MLX)
├── media-kit                  ← ffmpeg/ffprobe wrappers (audio extraction, probing)
├── transcription-kit          ← Whisper: MLX local + OpenAI cloud
└── content-kit                ← LLM features: summary, chapters, translate, YT pack, viral clips
```

Each crate has tests (`cargo test -p <crate>`).

## Data flow

```
User drops video / pastes YouTube URL
        │
        ▼
yt-dlp (if URL) → local file
        │
        ▼
ffprobe (duration)  ─┐
ffmpeg → mono 32kbps MP3 ──► Whisper (MLX local OR OpenAI cloud)
                                    │
                                    ▼
                       TranscriptEntry cached in AppState (HashMap<PathBuf, _>)
                                    │
     ┌──────────────┬────────────┬──┴──────────┬─────────────┬──────────────┐
     ▼              ▼            ▼             ▼             ▼              ▼
  Summary       Translate     Chapters      YT Pack     Viral Clips    Clean SRT
 (batched      (25s chunks,  (single-   (uses summary  (600s window,  (rule-based,
  30-min,       parallel     shot, LLM   as input)      LLM picks      no LLM)
  consolidated) JSON)        picks topics)               top 3-5)
```

All features share the cached transcript — the user transcribes once, then any feature tab runs instantly on top of it.

## Frontend ↔ Backend contract

Tauri commands exposed in [`src-tauri/src/commands/mod.rs`](../src-tauri/src/commands/mod.rs). Each frontend feature module calls `invoke("<command_name>", { request: {...} })` and subscribes to progress events via `listen("<event>")`.

Key events:

| Event | Emitted by | Payload |
|---|---|---|
| `mlx_whisper_progress` | transcribe | `{ current_ms, total_ms, percent }` |
| `translate_progress` | translate | `{ batch, total, percent }` |
| `yt_dlp_progress` | youtube | `{ percent, cached, label }` |

## State

`AppState` ([`src-tauri/src/state.rs`](../src-tauri/src/state.rs)) holds transcripts keyed by source path. Tauri injects it into every command via `State<'_, AppState>`. The frontend has its own reactive store at [`src/js/source-store.js`](../src/js/source-store.js).

Secrets (API keys) go through `KeyringSecretStore` ([`ai-kit`](../src-tauri/crates/ai-kit/src/secret_store.rs)) — OS keychain, not disk.

## Platform gates

- MLX-specific commands use `#[cfg(all(target_os = "macos", target_arch = "aarch64"))]` and provide stub error paths for other targets.
- `check_platform` command reports MLX availability so the frontend hides/disables the local-AI option.
