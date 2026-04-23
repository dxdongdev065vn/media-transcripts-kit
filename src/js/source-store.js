// Reactive store shared across feature views.
//
// Holds: source path, transcript snapshot, probe info, output status.
// Emits `change` events so views refresh when state changes.

const state = {
  path: "",
  transcript: null,
  summary: null,           // SummaryResult { text, language, style } — auto-generated after transcription
  probe: null,
  outputDir: "",           // "{stem}_output/" path created by Rust
  outputStatus: {},        // { transcript: true, translate: true, ... }
  aiReady: true,           // null=checking, true=ready, false=failed; cloud mode always true
};

// "local" = MLX (Apple Silicon), "cloud" = OpenAI
const aiConfig = {
  mode: "cloud",
  language: "Vietnamese",
  chatProviderId: "openai",
  chatProvider: "openAi",
  chatModel: "gpt-4o-mini",
  chatLabel: "OpenAI",
  transcriptionProviderId: "openai",
  transcriptionProvider: "openAi",
  transcriptionModel: "whisper-1",
  transcriptionLabel: "OpenAI",
};

const AI_DEFAULTS = {
  local: { provider: "mlx",    model: "mlx-community/Qwen3-14B-4bit" },
  cloud: { provider: "openAi", model: "gpt-4o-mini" },
};

const listeners = new Set();

function notify() {
  for (const fn of listeners) {
    try { fn({ ...state }); }
    catch (err) { console.error("source-store listener failed:", err); }
  }
}

export function getSource() { return { ...state }; }

export function setSourcePath(path) {
  const trimmed = (path || "").trim();
  if (trimmed === state.path) return;
  state.path = trimmed;
  state.transcript = null;
  state.summary = null;
  state.probe = null;
  state.outputDir = "";
  state.outputStatus = {};
  notify();
}

export function setSummary(summary) {
  state.summary = summary ? { ...summary } : null;
  notify();
}

export function getSummary() { return state.summary; }

export function setProbe(probe) {
  state.probe = probe ? { ...probe } : null;
  notify();
}

export function setTranscript(transcript) {
  state.transcript = transcript
    ? { language: transcript.language ?? null, segments: transcript.segments ?? [] }
    : null;
  notify();
}

export function setOutputDir(dir) {
  state.outputDir = dir || "";
}

export function setOutputStatus(statusMap) {
  state.outputStatus = { ...statusMap };
  notify();
}

// Mark a single output key as done (avoids re-scanning disk after every save).
export function markOutputDone(key) {
  state.outputStatus = { ...state.outputStatus, [key]: true };
  notify();
}

export function getAiConfig() {
  if (aiConfig.mode === "cloud") {
    return {
      mode: aiConfig.mode,
      providerId: aiConfig.chatProviderId,
      provider: aiConfig.chatProvider,
      model: aiConfig.chatModel,
      label: aiConfig.chatLabel,
      language: aiConfig.language,
    };
  }
  const { provider, model } = AI_DEFAULTS.local;
  return { mode: aiConfig.mode, provider, model, language: aiConfig.language };
}

export function getTranscriptionAiConfig() {
  if (aiConfig.mode === "cloud") {
    return {
      mode: aiConfig.mode,
      providerId: aiConfig.transcriptionProviderId,
      provider: aiConfig.transcriptionProvider,
      model: aiConfig.transcriptionModel,
      label: aiConfig.transcriptionLabel,
      language: aiConfig.language,
    };
  }
  return {
    mode: aiConfig.mode,
    providerId: "mlx",
    provider: "mlx",
    model: "mlx-community/whisper-large-v3-turbo",
    label: "MLX",
    language: aiConfig.language,
  };
}

export function setAiReady(ready) {
  state.aiReady = ready;
  notify();
}

export function setAiConfig(updates) {
  const prevMode = aiConfig.mode;
  Object.assign(aiConfig, updates);
  // Mode switch: cloud is always ready; local re-triggers check (source-manager handles it)
  if (updates.mode !== undefined && updates.mode !== prevMode) {
    state.aiReady = updates.mode === "cloud" ? true : null;
    notify();
  }
}

export function setAiCloudDefaults({
  chatProviderId,
  chatProvider,
  chatModel,
  chatLabel,
  transcriptionProviderId,
  transcriptionProvider,
  transcriptionModel,
  transcriptionLabel,
}) {
  aiConfig.chatProviderId = chatProviderId || "";
  aiConfig.chatProvider = chatProvider || "openAi";
  aiConfig.chatModel = chatModel || "gpt-4o-mini";
  aiConfig.chatLabel = chatLabel || "OpenAI";
  aiConfig.transcriptionProviderId = transcriptionProviderId || "";
  aiConfig.transcriptionProvider = transcriptionProvider || "openAi";
  aiConfig.transcriptionModel = transcriptionModel || "whisper-1";
  aiConfig.transcriptionLabel = transcriptionLabel || "";
  notify();
}

export function subscribe(fn) {
  listeners.add(fn);
  fn({ ...state });
  return () => listeners.delete(fn);
}
