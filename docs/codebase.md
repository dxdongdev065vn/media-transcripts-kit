# Codebase Map

Where to look when touching each concern.

## Top-level

```
creator_util/
├── src/                    # Frontend (HTML + JS)
├── src-tauri/              # Rust backend (Tauri shell + workspace crates)
├── assets/                 # Logos, icons (source PNG/SVG)
├── user-docs/              # End-user guide (Vietnamese, → GitHub Pages)
├── docs/                   # Developer docs (this folder)
├── .github/workflows/      # CI: release.yml, pages.yml
└── plans/                  # Local-only working notes (gitignored)
```

## Frontend — `src/`

```
src/
├── index.html              # Single page, all feature tabs
├── styles/main.css
└── js/
    ├── main.js             # Entry — wires sidebar + features
    ├── header.js           # Top bar
    ├── sidebar.js          # Source selector + AI config
    ├── source-store.js     # Reactive store (subscribe/notify)
    ├── source-manager.js   # AI engine lifecycle + yt-dlp progress banner
    ├── source-panel.js
    ├── updater.js          # tauri-plugin-updater hook
    ├── util.js             # DOM helpers, escapeHtml, SRT/TXT serialisers
    └── features/
        ├── transcribe.js
        ├── summary.js
        ├── translate.js
        ├── chapters.js
        ├── youtube-pack.js
        ├── viral-clips.js
        ├── settings.js
        └── provider-model-defaults.js
```

Each `features/*.js` module:
1. Grabs its tab's DOM nodes
2. Subscribes to `source-store` for availability gating
3. On button click: reads AI config + cached transcript, calls a Tauri command, renders results, auto-saves outputs

## Backend — `src-tauri/`

```
src-tauri/
├── Cargo.toml              # Workspace manifest
├── tauri.conf.json         # Bundle config (version, identifier, sidecars, icons, CSP)
├── capabilities/           # Tauri v2 permission scopes
├── binaries/               # Sidecar binaries (gitignored; fetched by CI)
├── icons/                  # Bundled app icons
├── entitlements.plist      # macOS entitlements (PyInstaller sidecars need library-validation disabled)
├── src/
│   ├── main.rs             # thin entry → lib.rs
│   ├── lib.rs              # Tauri builder: plugins, commands, state, path augmentation
│   ├── state.rs            # AppState (transcript cache)
│   └── commands/
│       ├── mod.rs          # command registration
│       ├── ai.rs           # provider key management
│       ├── content.rs      # summary, chapters, translate, YT pack, viral clips
│       ├── transcription.rs# MLX + OpenAI whisper
│       ├── mlx_server.rs   # lifecycle for local mlx_lm.server
│       ├── media.rs        # ffprobe wrapper
│       ├── files.rs        # path dialogs, write helpers
│       ├── meta.rs         # platform info
│       ├── output.rs       # output dir scan (re-hydrate features from disk)
│       └── youtube.rs      # yt-dlp download
└── crates/
    ├── creator-core/       # shared types
    ├── ai-kit/             # Provider trait + HTTP clients + keyring
    ├── media-kit/          # ffmpeg/ffprobe async wrappers
    ├── transcription-kit/  # MlxWhisperTranscriber, OpenAiWhisperTranscriber
    └── content-kit/        # batch.rs, summary.rs, chapters.rs, translate.rs, youtube_pack.rs, viral_clips.rs, transcript_filler_scan.rs
```

## Key modules by concern

| Concern | File |
|---|---|
| Add a new AI provider | `src-tauri/crates/ai-kit/src/providers/` + register in `AiProviderType` enum in `creator-core` |
| Change transcription format/bitrate | `src-tauri/src/commands/transcription.rs` → `prepare_audio` |
| Tweak feature prompts | `src-tauri/crates/content-kit/src/<feature>.rs` → `system_prompt` / `user_prompt` |
| Adjust feature post-processing (min-gap, dedupe, align) | same files, inside the `run` impl |
| Add a new Tauri command | implement in `src-tauri/src/commands/*.rs` + register in `commands/mod.rs` |
| Change bundled sidecars | `src-tauri/tauri.conf.json` → `bundle.externalBin`, CI downloads them in `.github/workflows/release.yml` |
| Modify UI layout | `src/index.html` (single file) + `src/styles/main.css` |
| Add a new feature tab | new `src/js/features/<name>.js` + HTML section in `index.html` + sidebar entry + backend command |
| Secret storage | `ai-kit/src/secret_store.rs` (OS keyring) |

## Conventions

- **Rust naming**: snake_case modules, files named after their primary type.
- **JS naming**: kebab-case files, camelCase vars.
- **Commit style**: conventional commits (`feat:`, `fix:`, `docs:`, `refactor:`, `chore:`). Don't use `chore` or `docs` for `.claude/` dir changes.
- **Error strings**: return `String` from Tauri commands (Tauri serialises `Result<T, String>` cleanly to JS).
- **Batch boundaries**: any content-kit feature that processes long transcripts chunks via `content_kit::batch::chunk_segments`.

## Non-obvious things

- **Path augmentation**: macOS GUI apps launched from Finder don't inherit Homebrew / pipx paths. `src-tauri/src/lib.rs::augmented_path()` injects `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin` into the child-process env whenever we spawn a Python sidecar.
- **mlx_whisper JSON location**: mlx_whisper writes output JSON to a tmp dir whose path varies by version. We glob the tmp dir for any `*.json` as a fallback.
- **yt-dlp exit 1 with file present**: recent YouTube changes cause yt-dlp to emit a rename error after the file has already landed at its final path. `youtube.rs` treats "non-zero exit + expected file exists" as success.
- **Format selector** for yt-dlp: `best[ext=mp4][acodec!=none][vcodec!=none]/18/best` — avoids formats that require the JS n-challenge solver.
- **OpenAI summary hint**: `max_completion_tokens` is used, NOT `max_tokens` (sending both triggers a 400).
