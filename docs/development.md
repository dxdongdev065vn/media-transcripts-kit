# Development

## Prerequisites

- **Rust** 1.80+ (`rustup`)
- **Node.js** 20+ with npm
- **ffmpeg** + **ffprobe** on PATH (for local dev; bundled in release builds)
- **Apple Silicon only, for local MLX mode**:
  ```bash
  pipx install mlx-lm mlx-whisper    # or pip install into a venv
  ```

## Clone + run

```bash
git clone git@github.com:phuc-nt/my-media-kit.git
cd my-media-kit
npm install
npm run dev       # launches `tauri dev` — hot reload for frontend, recompiles Rust on change
```

First build compiles the full workspace (~3-5 min). Subsequent builds are incremental.

## Sidecar binaries for local dev

Release CI fetches platform-specific `ffmpeg`/`ffprobe`/`yt-dlp` into `src-tauri/binaries/`. For local dev you can either:

1. Install them on PATH via Homebrew and rely on the fallback resolver in `src-tauri/src/lib.rs`.
2. Or copy binaries into `src-tauri/binaries/` matching the naming Tauri expects (`<name>-<target-triple>`).

The resolver tries (1) bundled sidecar → (2) env var `YT_DLP_BIN` etc. → (3) system PATH.

## Configure API keys for testing

Run the app → **Settings** tab → paste an OpenAI key → Save. Stored via OS keychain (Keychain on macOS, Credential Manager on Windows, Secret Service on Linux).

For MLX local testing: Settings → AI Mode → MLX. The app probes `mlx_whisper` + `mlx_lm.server` on launch; both must be installed.

## Build commands

| Command | Purpose |
|---|---|
| `npm run dev` | dev mode, hot reload |
| `npm run build` | production bundle (current platform only) |
| `cargo check --manifest-path src-tauri/Cargo.toml` | fast compile check, no link |
| `cargo test --manifest-path src-tauri/Cargo.toml` | workspace tests |
| `cargo test -p content-kit` | single crate tests |
| `cargo fmt --manifest-path src-tauri/Cargo.toml` | format Rust |

## Testing

- **Unit tests**: in-file `#[cfg(test)] mod tests` blocks for each crate.
- **Content-kit LLM tests**: use a `StubProvider` implementing the `Provider` trait — no network calls.
- **Manual UI testing**: `npm run dev`, drop a short `.mp4` or paste a YouTube URL, step through every tab.

No end-to-end / frontend automation yet. When adding a feature, write at least a `StubProvider`-based test for the content-kit runner.

## Debugging

- Frontend: right-click in app window → **Inspect Element** (WebKit devtools). `console.log` works.
- Backend: `RUST_LOG=debug npm run dev` — `tracing` spans print to terminal.
- Tauri IPC traffic: inspect `invoke` calls in devtools Network tab.
- MLX subprocess output: check terminal where `npm run dev` is running; `mlx_whisper` stdout/stderr is streamed.

## Adding a new feature

1. Write prompt + schema + runner in `content-kit/src/<feature>.rs`. Add tests using `StubProvider`.
2. Expose it as a Tauri command in `src-tauri/src/commands/content.rs`. Register in `commands/mod.rs`.
3. Add a `<section>` tab in `src/index.html` + sidebar entry.
4. Create `src/js/features/<feature>.js` — subscribe to store, wire up button, render results, auto-save to output dir.
5. Run `cargo check` → `npm run dev` → test manually.

## Troubleshooting local dev

| Symptom | Fix |
|---|---|
| `mlx_whisper not found` in app but works in terminal | GUI PATH doesn't include pipx bin — `augmented_path()` should handle it; check `~/.local/bin` exists |
| ffmpeg errors on transcribe | Install ffmpeg via Homebrew; app falls back to system PATH in dev |
| `keyring error` | On Linux you need `libsecret-1-dev` + an unlocked keyring (e.g. GNOME Keyring running) |
| Cargo.lock conflicts | `Cargo.lock` is gitignored; delete `src-tauri/Cargo.lock` and retry |
| macOS codesign fails locally | Expected — signing only runs in CI with certs. `npm run build` produces unsigned artifacts. |
