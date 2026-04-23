// Transcribe feature view.
//
// Backend (MLX vs OpenAI) is derived from the global AI engine setting.
// Models are fixed: mlx → whisper-large-v3-turbo, cloud → whisper-1.
// No per-feature config — everything comes from the source manager.

import {
  getSource,
  getAiConfig,
  getTranscriptionAiConfig,
  setTranscript,
  setSummary,
  subscribe,
  markOutputDone,
} from "../source-store.js";
import {
  deriveOutputPath,
  escapeHtml,
  formatMs,
  renderErrorBox,
  requireSource,
  segmentsToPlainText,
  segmentsToSrt,
  setStatus,
  showToast,
} from "../util.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const BACKEND_MODEL = {
  mlx: "mlx-community/whisper-large-v3-turbo",
  openai: "whisper-1",
};

export function initTranscribeView() {
  const results = document.getElementById("transcribe-results");
  const status = document.getElementById("transcribe-status");
  const btnRun = document.getElementById("btn-transcribe");
  const btnForce = document.getElementById("btn-transcribe-refresh");
  const btnSaveClean = document.getElementById("btn-transcribe-save-clean");

  const progressBox = document.getElementById("transcribe-progress");
  const progressBar = document.getElementById("transcribe-progress-bar");
  const progressLabel = document.getElementById("transcribe-progress-label");
  const progressValue = document.getElementById("transcribe-progress-value");

  const summaryBox  = document.getElementById("transcribe-summary");
  const summaryBody = document.getElementById("transcribe-summary-body");

  let currentTranscript = null;
  let running = false;
  let transcriptionGate = {
    loading: true,
    ready: false,
    message: "checking transcription provider…",
  };

  async function refreshTranscriptionGate(viewOverride = null) {
    try {
      const view = viewOverride || await invoke("ai_get_provider_settings");
      const hasCloudProvider = view.providers.some((provider) =>
        provider.enabled && provider.hasApiKey && provider.capabilities.includes("transcription"),
      );
      transcriptionGate = {
        loading: false,
        ready: hasCloudProvider,
        message: hasCloudProvider
          ? ""
          : "No cloud transcription provider is configured. Open Provider Settings and choose an Audio Transcription provider.",
      };
    } catch (err) {
      transcriptionGate = {
        loading: false,
        ready: false,
        message: `Cannot load transcription settings: ${String(err)}`,
      };
    }
  }

  function renderSummaryInline(summary) {
    if (!summary?.text) {
      summaryBox.hidden = true;
      summaryBody.innerHTML = "";
      return;
    }
    summaryBox.hidden = false;
    summaryBody.innerHTML = `
      <p class="hint">language: <code>${escapeHtml(summary.language)}</code>${summary.custom ? " · custom" : " · auto-generated"}</p>
      <pre class="summary-text">${escapeHtml(summary.text)}</pre>
    `;
  }

  function getBackend() {
    const { mode } = getTranscriptionAiConfig();
    return mode === "cloud" ? "openai" : "mlx";
  }

  function setRunning(on, label = "running whisper…") {
    running = on;
    btnRun.disabled = on;
    btnForce.disabled = on;
    if (on) {
      progressBox.hidden = false;
      progressLabel.textContent = label;
      progressBar.className = "progress-bar indeterminate";
      progressValue.textContent = "…";
      setStatus(status, label, "running");
    } else {
      progressBox.hidden = true;
      progressBar.className = "progress-bar";
      progressBar.style.width = "0%";
    }
  }

  function applyTranscript(out) {
    currentTranscript = out;
    setTranscript(out);
    renderSegments(out, results);
    btnSaveClean.disabled = !(out?.segments?.length > 0);
  }

  // Auto-summary as part of the Transcribe pipeline.
  // Status updates flow into the Transcribe status bar; result renders in
  // the inline Summary section below the transcript table.
  async function runAutoSummary(segments, source) {
    const { providerId, provider, model, language } = getAiConfig();
    summaryBox.hidden = false;
    summaryBody.innerHTML = `<p class="hint">generating summary…</p>`;
    setStatus(status, "summarizing…", "running");
    try {
      setStatus(status, "summarizing…", "running");
      const out = await invoke("content_summary", {
        request: {
          providerId: providerId || null,
          provider,
          model,
          segments,
          language: language || "English",
        },
      });
      const summary = { ...out };
      setSummary(summary);
      renderSummaryInline(summary);

      const summaryPath = deriveOutputPath(source.outputDir, "summary.md");
      if (summaryPath && summary.text) {
        await invoke("save_text_file", { path: summaryPath, content: summary.text }).catch(() => {});
        markOutputDone("summary");
      }
      setStatus(status, `${segments.length} segments + summary`, "ok");
    } catch (e) {
      console.error("auto-summary failed:", e);
      summaryBody.innerHTML = `<p class="hint" style="color:var(--err,#f88)">summary failed: ${escapeHtml(String(e))}</p>`;
      setStatus(status, `${segments.length} segments (summary failed)`, "err");
    }
  }

  // MLX streams segment-level progress events.
  listen("mlx_whisper_progress", (event) => {
    if (!running || getBackend() !== "mlx") return;
    const { percent, current_ms, total_ms } = event.payload || {};
    if (typeof percent !== "number") return;
    progressBar.className = "progress-bar";
    progressBar.style.width = `${percent.toFixed(1)}%`;
    progressValue.textContent = `${percent.toFixed(1)}%`;
    const cur = formatMs(current_ms || 0);
    const tot = total_ms ? formatMs(total_ms) : "?";
    progressLabel.textContent = `transcribing… ${cur} / ${tot}`;
  });

  async function run(force) {
    const source = getSource();
    if (!requireSource(source, status)) return;

    results.innerHTML = "";
    btnSaveClean.disabled = true;

    const backend = getBackend();
    const transcriptionConfig = getTranscriptionAiConfig();
    const model = transcriptionConfig.model || BACKEND_MODEL[backend];
    if (backend === "openai") {
      if (transcriptionGate.loading) {
        await refreshTranscriptionGate();
      }
      if (!transcriptionGate.ready) {
        renderErrorBox(results, transcriptionGate.message);
        setStatus(status, "configure transcription provider", "err");
        return;
      }
    }

    let label;
    if (backend === "openai") {
      const providerLabel = transcriptionConfig.label || "cloud transcription";
      label = force ? `re-running ${providerLabel}…` : `running ${providerLabel}…`;
    } else {
      const ready = await invoke("mlx_model_ready").catch(() => true);
      label = ready
        ? (force ? "re-running whisper…" : "running whisper…")
        : "downloading model (~1 GB) + transcribing…";
    }

    setRunning(true, label);

    try {
      let out;
      if (backend === "openai") {
        out = await invoke("openai_whisper_transcribe", {
          path: source.path,
          providerId: transcriptionConfig.providerId || null,
          language: null,
          model,
          force: !!force,
        });
      } else {
        out = await invoke("mlx_whisper_transcribe", {
          path: source.path, language: null, model, force: !!force,
        });
      }
      applyTranscript(out);
      setStatus(status, `${out.segments.length} segments${out.fromCache ? " (cached)" : ""}`, "ok");

      // Auto-save SRT and TXT to output directory.
      const srtPath = deriveOutputPath(source.outputDir, "transcript.srt");
      const txtPath = deriveOutputPath(source.outputDir, "transcript.txt");
      const srtContent = segmentsToSrt(out.segments);
      const txtContent = segmentsToPlainText(out.segments);
      await Promise.all([
        srtPath ? invoke("save_text_file", { path: srtPath, content: srtContent }).catch(() => {}) : Promise.resolve(),
        txtPath ? invoke("save_text_file", { path: txtPath, content: txtContent }).catch(() => {}) : Promise.resolve(),
      ]);
      markOutputDone("transcript");

      // Auto-generate summary as part of Transcribe — status + content shown here.
      await runAutoSummary(out.segments, source);

    } catch (e) {
      console.error(e);
      renderErrorBox(results, String(e));
      setStatus(status, "failed", "err");
    } finally {
      setRunning(false);
    }
  }

  btnRun.addEventListener("click", () => run(false));
  btnForce.addEventListener("click", () => run(true));
  btnSaveClean.addEventListener("click", async () => {
    if (!currentTranscript?.segments?.length) return;
    const source = getSource();
    if (!source?.path) return;
    try {
      const cleaned = await invoke("content_clean_transcript", {
        segments: currentTranscript.segments,
      });
      const target = deriveOutputPath(source.outputDir, "clean.srt");
      const content = segmentsToSrt(cleaned);
      const written = await invoke("save_text_file", { path: target, content });
      markOutputDone("clean");
      showToast(`saved clean → ${written}`, "ok");
    } catch (e) {
      console.error(e);
      showToast(`clean save failed: ${e}`, "err");
    }
  });

  subscribe(async (state) => {
    if (running) return;
    const backend = getBackend();
    const backendReady = backend === "mlx" || transcriptionGate.ready;
    btnRun.disabled = !state.path || !backendReady;
    btnForce.disabled = !state.path || !backendReady;
    // Always reflect summary state inline (even after re-opening the app).
    renderSummaryInline(state.summary);
    if (!state.path) {
      results.innerHTML = `<p class="hint">no source selected</p>`;
      currentTranscript = null;
      btnSaveClean.disabled = true;
      summaryBox.hidden = true;
      return;
    }
    if (state.transcript) {
      applyTranscript({ ...state.transcript, fromCache: true });
      if (backend === "openai" && !transcriptionGate.ready) {
        setStatus(status, "configure transcription provider", "err");
      }
      // Auto-load cached summary.md from disk if no summary in store yet.
      if (!state.summary && state.outputStatus?.summary) {
        try {
          const text = await invoke("read_output_file", { sourcePath: state.path, filename: "summary.md" });
          if (text) setSummary({ text, language: getAiConfig().language || "English", custom: false });
        } catch (_) {}
      }
      return;
    }
    try {
      const transcriptionConfig = getTranscriptionAiConfig();
      const hit = await invoke("get_cached_transcript", {
        path: state.path,
        backend: backend === "openai" ? "openai-compatible-audio" : "mlx",
        providerId: transcriptionConfig.providerId || null,
        model: transcriptionConfig.model || BACKEND_MODEL[backend],
      });
      if (hit) {
        applyTranscript(hit);
        setStatus(status, `${hit.segments.length} segments (cached)`, "ok");
      } else {
        if (getBackend() === "openai" && !transcriptionGate.loading && !transcriptionGate.ready) {
          results.innerHTML = `<p class="hint">${escapeHtml(transcriptionGate.message)}</p>`;
          setStatus(status, "configure transcription provider", "err");
        } else {
          results.innerHTML = `<p class="hint">not transcribed yet — hit Transcribe above</p>`;
        }
        currentTranscript = null;
        btnSaveClean.disabled = true;
      }
    } catch (e) {
      renderErrorBox(results, String(e));
    }
  });

  window.addEventListener("provider-settings-changed", (event) => {
    refreshTranscriptionGate(event.detail || null).catch((err) => {
      console.error("transcription provider refresh failed:", err);
    });
  });

  refreshTranscriptionGate().catch((err) => {
    console.error("initial transcription provider check failed:", err);
  });
}

function renderSegments(out, container) {
  const lang = out.language
    ? `<p class="hint">language: <code>${escapeHtml(out.language)}</code></p>`
    : "";
  const rows = out.segments
    .map((s) =>
      `<tr><td>${formatMs(s.start_ms)}</td><td>${formatMs(s.end_ms)}</td><td>${escapeHtml(s.text)}</td></tr>`)
    .join("");
  container.innerHTML = `
    ${lang}
    <table>
      <thead><tr><th>Start</th><th>End</th><th>Text</th></tr></thead>
      <tbody>${rows}</tbody>
    </table>
  `;
}
