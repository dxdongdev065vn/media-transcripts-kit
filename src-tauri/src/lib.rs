//! My Media Kit — Tauri app entry point.
//!
//! Keeps top-level concerns (command registration, state setup) here.
//! Feature implementations live in `commands/` and the `crates/` under
//! src-tauri so the app boundary stays thin and everything else stays
//! testable outside Tauri.

mod commands;
mod state;

pub use state::{AppState, TranscriptEntry};

use commands::mlx_server::kill_server_pid;

fn sidecar_is_usable(path: &std::path::Path, version_arg: &str) -> bool {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return false,
    };
    if !metadata.is_file() {
        return false;
    }
    // Ignore empty placeholder files created only to satisfy build-time
    // bundler checks; they are not runnable binaries.
    if metadata.len() == 0 {
        return false;
    }
    match std::process::Command::new(path)
        .arg(version_arg)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(err) => {
            // Windows returns OS error 193 for invalid executables. This is
            // common in local dev when placeholder sidecar files exist, so we
            // suppress noisy WARN logs and quietly fall back to PATH.
            if cfg!(windows) && err.raw_os_error() == Some(193) {
                tracing::debug!("ignoring placeholder sidecar {}: {}", path.display(), err);
            } else {
                tracing::debug!("ignoring unusable sidecar {}: {}", path.display(), err);
            }
            false
        }
    }
}

/// Build the Tauri app and run it. Called from `main.rs` (desktop) and from
/// mobile entry points (if/when added).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .setup(|_app| {
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::INFO)
                .try_init()
                .ok();
            // Point media-kit at the bundled ffmpeg/ffprobe sidecars so the
            // user does not need a system install. Tauri places `externalBin`
            // entries next to the main executable (Contents/MacOS/ on macOS,
            // same dir as the .exe on Windows). If the bundled file is missing
            // (e.g. local `cargo run`), we fall back to PATH lookup.
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    let resolve = |name: &str| {
                        let suffix = if cfg!(windows) { ".exe" } else { "" };
                        dir.join(format!("{name}{suffix}"))
                    };
                    let ffmpeg = resolve("ffmpeg");
                    let ffprobe = resolve("ffprobe");
                    let ytdlp = resolve("yt-dlp");
                    if sidecar_is_usable(&ffmpeg, "-version") {
                        std::env::set_var("FFMPEG", ffmpeg);
                    }
                    if sidecar_is_usable(&ffprobe, "-version") {
                        std::env::set_var("FFPROBE", ffprobe);
                    }
                    if sidecar_is_usable(&ytdlp, "--version") {
                        std::env::set_var("YT_DLP_BIN", ytdlp);
                    }
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                use tauri::Manager;
                let state = window.app_handle().state::<AppState>();
                kill_server_pid(&state);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::platform_info,
            commands::media_probe,
            commands::ai_provider_status,
            commands::ai_has_api_key,
            commands::ai_set_api_key,
            commands::ai_delete_api_key,
            commands::ai_ping,
            commands::ai_get_provider_settings,
            commands::ai_save_provider_settings,
            commands::ai_test_provider,
            commands::mlx_whisper_transcribe,
            commands::openai_whisper_transcribe,
            commands::import_srt,
            commands::content_summary,
            commands::content_chapters,
            commands::content_translate,
            commands::content_youtube_pack,
            commands::content_viral_clips,
            commands::content_clean_transcript,
            commands::get_cached_transcript,
            commands::clear_cache,
            commands::check_platform,
            commands::mlx_model_ready,
            commands::ensure_output_dir,
            commands::scan_output_status,
            commands::list_output_files,
            commands::load_transcript_from_output,
            commands::read_output_file,
            commands::save_text_file,
            commands::yt_dlp_download,
            commands::ensure_mlx_lm_server,
            commands::mlx_server_is_ready,
            commands::stop_mlx_lm_server,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
