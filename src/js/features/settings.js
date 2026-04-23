import { updater } from "../updater.js";
import { setAiCloudDefaults } from "../source-store.js";

const { invoke } = window.__TAURI__.core;

let providerState = [];
let flowDefaults = {
  defaultTextProviderId: "",
  defaultTranscriptionProviderId: "",
};

function setStatus(text, kind = "") {
  const el = document.getElementById("ai-status");
  if (!el) return;
  el.textContent = text;
  el.className = "status " + kind;
}

function setModalStatus(text, kind = "") {
  const el = document.getElementById("provider-settings-feedback");
  if (!el) return;
  el.textContent = text;
  el.className = "status " + kind;
}

function escapeHtml(value) {
  return String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function capabilityBadges(provider) {
  const set = new Set(provider.capabilities || []);
  const badges = [];
  if (set.has("text")) badges.push("Chat Completions");
  if (set.has("transcription")) badges.push("Audio Transcription");
  if (set.has("text") && !set.has("transcription")) badges.push("Text-only");
  if (set.has("vision")) badges.push("Vision");
  if (set.has("image")) badges.push("Image");
  return badges;
}

function providerTypeOptions(current) {
  return [
    ["openAi", "OpenAI-compatible"],
    ["openRouter", "OpenRouter"],
    ["claude", "Claude"],
    ["gemini", "Gemini"],
    ["ollama", "Ollama"],
  ]
    .map(([value, label]) => `<option value="${value}" ${current === value ? "selected" : ""}>${label}</option>`)
    .join("");
}

function providerDraft() {
  return {
    id: "",
    label: "New Provider",
    providerType: "openAi",
    baseUrl: "https://api.openai.com",
    defaultModel: "gpt-4o-mini",
    enabled: false,
    priority: providerState.length,
    capabilities: ["text"],
    apiKeyRef: "",
    hasApiKey: false,
    apiKey: "",
    clearApiKey: false,
  };
}

function normalizeProvider(provider, index) {
  return {
    id: provider.id || "",
    label: provider.label || "Provider",
    providerType: provider.providerType || "openAi",
    baseUrl: provider.baseUrl || "",
    defaultModel: provider.defaultModel || "gpt-4o-mini",
    enabled: !!provider.enabled,
    priority: index,
    capabilities: Array.isArray(provider.capabilities) && provider.capabilities.length
      ? provider.capabilities.slice()
      : ["text"],
    apiKeyRef: provider.apiKeyRef || "",
    hasApiKey: !!provider.hasApiKey,
    apiKey: provider.apiKey || "",
    clearApiKey: !!provider.clearApiKey,
  };
}

function serializeProvider(provider, index) {
  return {
    id: provider.id || null,
    label: provider.label,
    providerType: provider.providerType,
    baseUrl: provider.baseUrl || null,
    defaultModel: provider.defaultModel,
    enabled: provider.enabled,
    priority: index,
    capabilities: provider.capabilities,
    apiKeyRef: provider.apiKeyRef || null,
    apiKey: provider.apiKey || null,
    clearApiKey: !!provider.clearApiKey,
  };
}

function renderProviderSummary(view) {
  const summary = document.getElementById("provider-settings-summary");
  const defaults = document.getElementById("provider-default-summary");
  if (!summary || !defaults) return;

  const chatProvider = view.providers.find((provider) => provider.id === view.defaultTextProviderId);
  const transcriptionProvider = view.providers.find((provider) => provider.id === view.defaultTranscriptionProviderId);
  const chatSummary = chatProvider
    ? `Chat Completions: ${chatProvider.label} (${view.defaultTextModel || chatProvider.defaultModel})`
    : "Chat Completions: not configured";
  const transcriptionSummary = transcriptionProvider
    ? `Transcription: ${transcriptionProvider.label} (${view.defaultTranscriptionModel || transcriptionProvider.defaultModel})`
    : "Transcription: not configured";
  defaults.textContent = `${chatSummary} | ${transcriptionSummary}`;

  summary.innerHTML = view.providers
    .map((provider) => {
      const caps = capabilityBadges(provider).map((cap) => `<span class="provider-chip">${escapeHtml(cap)}</span>`).join("");
      return `
        <div class="provider-summary-card">
          <div class="provider-summary-head">
            <strong>${escapeHtml(provider.label)}</strong>
            <span class="provider-pill ${provider.enabled ? "ok" : ""}">${provider.enabled ? "enabled" : "disabled"}</span>
          </div>
          <div class="provider-summary-meta">
            <span>${escapeHtml(provider.providerType)}</span>
            <span>${escapeHtml(provider.defaultModel)}</span>
            <span>${provider.hasApiKey ? "key stored" : "no key"}</span>
          </div>
          <div class="provider-summary-caps">${caps}</div>
        </div>
      `;
    })
    .join("");
}

function applyCloudDefaults(view) {
  const chatProvider = view.providers.find((provider) => provider.id === view.defaultTextProviderId);
  const transcriptionProvider = view.providers.find((provider) => provider.id === view.defaultTranscriptionProviderId);
  setAiCloudDefaults({
    chatProviderId: chatProvider?.id,
    chatProvider: chatProvider?.providerType,
    chatModel: view.defaultTextModel || chatProvider?.defaultModel,
    chatLabel: chatProvider?.label,
    transcriptionProviderId: transcriptionProvider?.id,
    transcriptionProvider: transcriptionProvider?.providerType,
    transcriptionModel: view.defaultTranscriptionModel || transcriptionProvider?.defaultModel,
    transcriptionLabel: transcriptionProvider?.label,
  });
}

function flowOptions(capability, selectedId) {
  const eligible = providerState.filter((provider) => provider.enabled && provider.capabilities.includes(capability));
  const base = [`<option value="">Auto pick enabled ${capability} provider</option>`];
  return base.concat(
    eligible.map((provider) =>
      `<option value="${provider.id}" ${provider.id === selectedId ? "selected" : ""}>${escapeHtml(provider.label)}</option>`,
    ),
  ).join("");
}

function renderFlowSelectors() {
  const chatSelect = document.getElementById("provider-default-text");
  const transcriptionSelect = document.getElementById("provider-default-transcription");
  if (!chatSelect || !transcriptionSelect) return;
  chatSelect.innerHTML = flowOptions("text", flowDefaults.defaultTextProviderId);
  transcriptionSelect.innerHTML = flowOptions("transcription", flowDefaults.defaultTranscriptionProviderId);
}

function renderProviderEditor() {
  const container = document.getElementById("provider-settings-list");
  if (!container) return;

  container.innerHTML = providerState
    .map((provider, index) => {
      const capabilitySet = new Set(provider.capabilities);
      return `
        <section class="provider-editor-card" data-index="${index}">
          <div class="provider-editor-head">
            <strong>${escapeHtml(provider.label || "Provider")}</strong>
            <div class="provider-editor-actions">
              <button type="button" class="ghost small" data-action="move-up">Up</button>
              <button type="button" class="ghost small" data-action="move-down">Down</button>
              <button type="button" class="ghost small" data-action="test-chat">Test chat</button>
              ${capabilitySet.has("transcription") ? '<button type="button" class="ghost small" data-action="test-audio">Test audio</button>' : ""}
              <button type="button" class="ghost small" data-action="remove">Remove</button>
            </div>
          </div>
          <div class="provider-grid">
            <label>
              <span>Name</span>
              <input data-field="label" type="text" value="${escapeHtml(provider.label)}" />
            </label>
            <label>
              <span>Type</span>
              <select data-field="providerType">
                ${providerTypeOptions(provider.providerType)}
              </select>
            </label>
            <label class="provider-grid-wide">
              <span>Base URL</span>
              <input data-field="baseUrl" type="text" value="${escapeHtml(provider.baseUrl)}" placeholder="https://api.example.com" />
            </label>
            <label>
              <span>Default model</span>
              <input data-field="defaultModel" type="text" value="${escapeHtml(provider.defaultModel)}" />
            </label>
            <label class="provider-toggle">
              <span>Enabled</span>
              <input data-field="enabled" type="checkbox" ${provider.enabled ? "checked" : ""} />
            </label>
          </div>
          <div class="provider-capabilities">
            <label><input data-capability="text" type="checkbox" ${capabilitySet.has("text") ? "checked" : ""} /> Text</label>
            <label><input data-capability="transcription" type="checkbox" ${capabilitySet.has("transcription") ? "checked" : ""} /> Transcription</label>
            <label><input data-capability="vision" type="checkbox" ${capabilitySet.has("vision") ? "checked" : ""} /> Vision</label>
            <label><input data-capability="image" type="checkbox" ${capabilitySet.has("image") ? "checked" : ""} /> Image</label>
          </div>
          <div class="provider-key-row">
            <input data-field="apiKey" type="password" placeholder="${provider.hasApiKey && !provider.clearApiKey ? "Stored in system keyring" : "Paste API key"}" value="" />
            <button type="button" class="ghost small" data-action="clear-key">${provider.hasApiKey && !provider.clearApiKey ? "Clear stored key" : "Reset key state"}</button>
          </div>
        </section>
      `;
    })
    .join("");
}

async function refreshProviderSettings() {
  const view = await invoke("ai_get_provider_settings");
  providerState = view.providers.map((provider, index) => normalizeProvider(provider, index));
  flowDefaults = {
    defaultTextProviderId: view.defaultTextProviderId || "",
    defaultTranscriptionProviderId: view.defaultTranscriptionProviderId || "",
  };
  applyCloudDefaults(view);
  renderProviderSummary(view);
  renderProviderEditor();
  renderFlowSelectors();
  return view;
}

function moveProvider(index, direction) {
  const nextIndex = index + direction;
  if (nextIndex < 0 || nextIndex >= providerState.length) return;
  const next = providerState.slice();
  const [item] = next.splice(index, 1);
  next.splice(nextIndex, 0, item);
  providerState = next.map((provider, idx) => normalizeProvider(provider, idx));
  renderProviderEditor();
  renderFlowSelectors();
}

function removeProvider(index) {
  providerState.splice(index, 1);
  providerState = providerState.map((provider, idx) => normalizeProvider(provider, idx));
  if (!providerState.some((provider) => provider.id === flowDefaults.defaultTextProviderId && provider.enabled && provider.capabilities.includes("text"))) {
    flowDefaults.defaultTextProviderId = "";
  }
  if (!providerState.some((provider) => provider.id === flowDefaults.defaultTranscriptionProviderId && provider.enabled && provider.capabilities.includes("transcription"))) {
    flowDefaults.defaultTranscriptionProviderId = "";
  }
  renderProviderEditor();
  renderFlowSelectors();
}

function updateProvider(index, updater) {
  providerState[index] = normalizeProvider(updater({ ...providerState[index] }), index);
}

async function saveProviders() {
  setModalStatus("saving...", "running");
  const input = {
    defaultTextProviderId: flowDefaults.defaultTextProviderId || null,
    defaultTranscriptionProviderId: flowDefaults.defaultTranscriptionProviderId || null,
    providers: providerState.map((provider, index) => serializeProvider(provider, index)),
  };
  const view = await invoke("ai_save_provider_settings", { input });
  providerState = view.providers.map((provider, index) => normalizeProvider(provider, index));
  flowDefaults = {
    defaultTextProviderId: view.defaultTextProviderId || "",
    defaultTranscriptionProviderId: view.defaultTranscriptionProviderId || "",
  };
  applyCloudDefaults(view);
  renderProviderSummary(view);
  renderProviderEditor();
  renderFlowSelectors();
  setStatus("provider settings saved", "ok");
  setModalStatus("saved", "ok");
  window.dispatchEvent(new CustomEvent("provider-settings-changed", { detail: view }));
}

async function testProvider(index, capability = "text") {
  const provider = serializeProvider(providerState[index], index);
  const label = capability === "transcription" ? "audio transcription" : "chat";
  setModalStatus(`testing ${provider.label} ${label}...`, "running");
  const result = await invoke("ai_test_provider", {
    input: { provider, capability },
  });
  setModalStatus(result.message, result.ok ? "ok" : "err");
}

export function initSettingsView() {
  const settingsView = document.querySelector('[data-view="settings"]');
  const modal = document.getElementById("provider-settings-modal");
  const openButtons = [
    document.getElementById("btn-provider-settings"),
    document.getElementById("btn-open-provider-settings-tab"),
  ].filter(Boolean);
  const btnAdd = document.getElementById("btn-provider-add");
  const btnSave = document.getElementById("btn-provider-save");
  const btnClose = document.getElementById("btn-provider-close");
  const providerList = document.getElementById("provider-settings-list");
  const defaultTextSelect = document.getElementById("provider-default-text");
  const defaultTranscriptionSelect = document.getElementById("provider-default-transcription");

  let loaded = false;
  async function ensureLoaded() {
    if (loaded) return;
    loaded = true;
    try {
      await refreshProviderSettings();
    } catch (e) {
      loaded = false;
      setStatus(String(e), "err");
      throw e;
    }
  }

  async function openModal() {
    await ensureLoaded();
    setModalStatus("", "");
    if (typeof modal.showModal === "function") modal.showModal();
  }

  openButtons.forEach((button) => {
    button.addEventListener("click", () => openModal().catch((e) => setStatus(String(e), "err")));
  });

  btnClose?.addEventListener("click", (event) => {
    event.preventDefault();
    modal.close();
  });

  btnAdd?.addEventListener("click", () => {
    providerState.push(providerDraft());
    renderProviderEditor();
    renderFlowSelectors();
  });

  btnSave?.addEventListener("click", () => {
    saveProviders().catch((e) => {
      setStatus(String(e), "err");
      setModalStatus(String(e), "err");
    });
  });

  providerList?.addEventListener("input", (event) => {
    const card = event.target.closest(".provider-editor-card");
    if (!card) return;
    const index = Number(card.dataset.index);
    const field = event.target.dataset.field;
    if (!field) return;

    updateProvider(index, (provider) => {
      if (field === "enabled") {
        provider.enabled = !!event.target.checked;
      } else {
        provider[field] = event.target.value;
      }
      if (field === "apiKey") {
        provider.clearApiKey = false;
      }
      return provider;
    });
    renderFlowSelectors();
  });

  providerList?.addEventListener("change", (event) => {
    const card = event.target.closest(".provider-editor-card");
    if (!card) return;
    const index = Number(card.dataset.index);

    if (event.target.dataset.capability) {
      updateProvider(index, (provider) => {
        const capability = event.target.dataset.capability;
        const set = new Set(provider.capabilities);
        if (event.target.checked) set.add(capability);
        else set.delete(capability);
        provider.capabilities = Array.from(set);
        return provider;
      });
      renderFlowSelectors();
      return;
    }

    const field = event.target.dataset.field;
    if (!field) return;
    updateProvider(index, (provider) => {
      provider[field] = field === "enabled" ? !!event.target.checked : event.target.value;
      return provider;
    });
    renderFlowSelectors();
  });

  defaultTextSelect?.addEventListener("change", (event) => {
    flowDefaults.defaultTextProviderId = event.target.value || "";
  });

  defaultTranscriptionSelect?.addEventListener("change", (event) => {
    flowDefaults.defaultTranscriptionProviderId = event.target.value || "";
  });

  providerList?.addEventListener("click", (event) => {
    const button = event.target.closest("button[data-action]");
    if (!button) return;
    const card = button.closest(".provider-editor-card");
    if (!card) return;
    const index = Number(card.dataset.index);
    const action = button.dataset.action;

    if (action === "move-up") moveProvider(index, -1);
    if (action === "move-down") moveProvider(index, 1);
    if (action === "remove") removeProvider(index);
    if (action === "clear-key") {
      updateProvider(index, (provider) => {
        provider.apiKey = "";
        provider.clearApiKey = true;
        provider.hasApiKey = false;
        return provider;
      });
      renderProviderEditor();
      renderFlowSelectors();
    }
    if (action === "test-chat") {
      testProvider(index, "text").catch((e) => setModalStatus(String(e), "err"));
    }
    if (action === "test-audio") {
      testProvider(index, "transcription").catch((e) => setModalStatus(String(e), "err"));
    }
  });

  const observer = new MutationObserver(() => {
    if (settingsView.classList.contains("active")) {
      ensureLoaded().catch((e) => setStatus(String(e), "err"));
    }
  });
  observer.observe(settingsView, { attributes: true, attributeFilter: ["class"] });

  const statusText = document.getElementById("update-status-text");
  const btnCheck = document.getElementById("btn-check-update");
  const availableBox = document.getElementById("update-available");
  const versionEl = document.getElementById("update-version");
  const notesEl = document.getElementById("update-notes");
  const btnDoUpdate = document.getElementById("btn-do-update");
  const progressBox = document.getElementById("update-progress");
  const progressBar = document.getElementById("update-progress-bar");
  const progressPct = document.getElementById("update-progress-pct");

  updater.onUpdateFound = (version, notes) => {
    statusText.textContent = `Update available: v${version}`;
    versionEl.textContent = `v${version}`;
    notesEl.textContent = notes || "";
    availableBox.hidden = false;
  };

  updater.onCheckComplete = (hasUpdate) => {
    if (!hasUpdate) statusText.textContent = "You're on the latest version";
    btnCheck.disabled = false;
  };

  updater.onError = () => {
    statusText.textContent = "Update check failed, try again later";
    btnCheck.disabled = false;
  };

  btnCheck.addEventListener("click", () => {
    statusText.textContent = "Checking for updates...";
    btnCheck.disabled = true;
    availableBox.hidden = true;
    updater.checkForUpdates();
  });

  btnDoUpdate.addEventListener("click", async () => {
    btnDoUpdate.disabled = true;
    progressBox.hidden = false;
    progressBar.style.width = "0%";
    progressPct.textContent = "0%";

    try {
      await updater.downloadAndInstall((downloaded, total) => {
        if (total > 0) {
          const pct = Math.round((downloaded / total) * 100);
          progressBar.style.width = `${pct}%`;
          progressPct.textContent = `${pct}%`;
        }
      });
      statusText.textContent = "Update installed, restarting...";
      try {
        const relaunch = window.__TAURI__?.process?.relaunch;
        if (relaunch) await relaunch();
      } catch (_) {
        statusText.textContent = "Update installed, please restart the app manually";
      }
    } catch (e) {
      statusText.textContent = `Update failed: ${e}`;
      btnDoUpdate.disabled = false;
      progressBox.hidden = true;
    }
  });

  setTimeout(() => updater.checkForUpdates(), 5000);
  ensureLoaded().catch(() => {});
}
