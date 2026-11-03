import { AgentCommands, AGENT_API_VERSION } from '../core/agent-commands.mjs';
import { DEFAULT_AGENT_PROMPT, migrateAgentPrompt } from '../core/agent-prompt.mjs';
import { EditorAgent } from '../application/agent.mjs';
import { createAgentEditorCommands } from '../application/agent-editor.mjs';
import { AgentProviders, createCompatibleProvider } from '../platform/agent-provider.mjs';
import { localPreferences } from '../platform/local-preferences.mjs';
import { AgentControls } from './agent-controls.mjs';
import { AgentDialogues } from './agent-dialogues.mjs';
import { AgentCredentials } from '../platform/agent-credentials.mjs';
import { AgentWorkspace } from '../platform/agent-workspace.mjs';
import { AgentCapture } from './agent-capture.mjs';
import { t } from '../i18n.mjs';
import { AGENT_ROUTE_PRESETS, AGENT_PROFILE_FIELDS, agentProfileSettings, agentReasoningOptions } from '../core/agent-routing.mjs';
import { showAgentConfiguration } from './agent-configuration.mjs';

const SETTINGS = 'rpe-next-agent-v1';
export const DEFAULT_AGENT_SETTINGS = Object.freeze({ provider: 'deepseek', model: 'deepseek-flash', permission: 'ask', thinking: true, maxTokens: 65536, maxRounds: 128, autoCompact: true, contextBudget: 0, fontSize: 14, rememberKey: true, prompt: DEFAULT_AGENT_PROMPT, uiVersion: 5 });
export function agentSettings(saved = {}) {
  const settings = Object.fromEntries(Object.entries(DEFAULT_AGENT_SETTINGS).map(([name, fallback]) => [name, typeof saved?.[name] === typeof fallback ? saved[name] : fallback]));
  if ((saved.uiVersion ?? 0) < 4) {
    if (saved.maxRounds === 24) settings.maxRounds = 128;
    if (saved.contextBudget === 131072) settings.contextBudget = 0;
  }
  if (!Number.isInteger(settings.maxRounds) || settings.maxRounds < 1 || settings.maxRounds > 4096) settings.maxRounds = 128;
  if (!Number.isInteger(settings.contextBudget) || settings.contextBudget !== 0 && (settings.contextBudget < 8192 || settings.contextBudget > 16777216)) settings.contextBudget = 0;
  if (!saved?.uiVersion && saved?.maxTokens === 8192) settings.maxTokens = 65536;
  Object.assign(settings, agentProfileSettings({ ...settings, ...saved, ...(!Array.isArray(saved.profiles) ? { maxTokens: settings.maxTokens, contextBudget: settings.contextBudget } : {}) }));
  settings.prompt = migrateAgentPrompt(settings.prompt);
  settings.uiVersion = 5; return settings;
}
const dataUrl = blob => new Promise((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(reader.result); reader.onerror = () => reject(reader.error); reader.readAsDataURL(blob); });

export class AgentPanel {
  constructor(editor, { activate, writeAsset, capture, notify, prependSilence }) {
    Object.assign(this, { editor, activate, notify }); this.commands = new AgentCommands(); this.providers = new AgentProviders(); for (const preset of AGENT_ROUTE_PRESETS) this.providers.register(createCompatibleProvider(preset));
    let saved; try { saved = JSON.parse(localPreferences.getItem(SETTINGS) ?? '{}'); } catch { saved = {}; }
    this.settings = agentSettings(saved);
    if (!Number.isInteger(this.settings.fontSize) || this.settings.fontSize < 10 || this.settings.fontSize > 24) this.settings.fontSize = DEFAULT_AGENT_SETTINGS.fontSize;
    if (typeof saved?.prompt === 'string' && [3850, 4771, 5691, 5814, 6145, 6368, 6672, 6885, 8116, 8250].includes(saved.prompt.length)) { let fingerprint = 2166136261; for (const character of saved.prompt) fingerprint = Math.imul(fingerprint ^ character.charCodeAt(0), 16777619); if ([[3850, 2788652168], [4771, 365650397], [5691, 381708962], [5814, 3991041475], [6145, 993312695], [6368, 920097850], [6672, 694240356], [6885, 995422563], [8116, 2954976899], [8250, 3185019651]].some(([length, value]) => saved.prompt.length === length && (fingerprint >>> 0) === value)) this.settings.prompt = DEFAULT_AGENT_PROMPT; }
    this.key = ''; this.memoryKeys = new Map(); this.credentials = new AgentCredentials(); this.waiting = new Set(); this.host = document.querySelector('#ai-panel'); this.tab = document.querySelector('#ai-tab'); this.controls = new AgentControls();
    this.applyAppearance();
    this.dialogues = new AgentDialogues(this);
    const network = async (method, value, signal) => {
      const config = this.config(), provider = this.providers.get(config.provider); if (typeof provider[method] !== 'function') throw Error(t('agent-panel.s0b10b17'));
      const result = await provider[method](config, value, signal), encoded = JSON.stringify(result);
      return encoded.length > 45000 ? { content: encoded.slice(0, 45000), truncated: true } : result;
    };
    this.workspace = new AgentWorkspace(() => editor.agentWorkspaceId);
    this.capture = new AgentCapture({ controls: this.controls, refresh: () => editor.invalidate() });
    this.adapter = createAgentEditorCommands(this.commands, editor, { controls: this.controls, network, ask: (args, signal) => this.dialogues.ask(args, signal), capture, captureEditor: (args, signal) => this.capture.capture(args, signal), prependSilence, workspace: this.workspace });
    editor.agent = { apiVersion: AGENT_API_VERSION, commands: this.commands, providers: this.providers, allowedPanes: () => [...document.querySelectorAll('[data-panel]')].map(node => node.dataset.panel),
      capabilities: () => this.capabilities(), workspace: this.workspace, flush: () => this.flush(), dataUrl, writeAsset, open: () => this.open() };
    this.agent = new EditorAgent({ commands: this.commands, providers: this.providers, context: () => ({ session: editor.hasDocument ? editor.session : null, summary: this.adapter.summary() }),
      config: () => this.config(), approve: (plan, signal) => this.dialogues.approve(plan, signal), prompt: () => this.settings.prompt });
    this.dialogues.bind(this.agent); this.dialogues.updateStatus();
    this.credentialsReady = this.loadProfileKey();
    this.providers.addEventListener('change', () => this.dialogues.updateRouting());
    Object.defineProperty(editor.agent, 'ready', { get: () => Promise.all([this.credentialsReady, this.dialogues.ready]) });
    document.querySelector('#ai-tool').onclick = () => this.open(); this.tab.onclick = () => this.open();
  }
  config() {
    const preferred = this.settings.thinking === false ? 'none' : this.settings.reasoningEffort, reasoningEffort = this.reasoningOptions().includes(preferred) ? preferred : 'auto';
    return { ...this.settings, reasoningEffort, thinking: reasoningEffort !== 'none', key: this.key, profileName: this.settings.profiles.find(profile => profile.id === this.settings.activeProfile)?.name };
  }
  capabilities() {
    const config = this.settings, metadata = this.modelMetadata;
    return { ...this.providers.entries.get(config.provider)?.capabilities?.(config.model), ...(metadata?.profile === config.activeProfile && metadata.model === config.model && metadata.baseUrl === config.baseUrl ? metadata.value : {}) };
  }
  reasoningOptions() { return agentReasoningOptions(this.settings, this.capabilities()); }
  async refreshCapabilities() {
    const config = this.config(), provider = this.providers.entries.get(config.provider), generation = this.metadataGeneration = (this.metadataGeneration ?? 0) + 1;
    try {
      const value = config.key && provider?.modelInfo ? await provider.modelInfo(config) : {};
      if (generation !== this.metadataGeneration || this.key !== config.key) return;
      this.modelMetadata = { profile: config.activeProfile, model: config.model, baseUrl: config.baseUrl, value }; this.dialogues.updateRouting(); this.dialogues.updateContextUsage();
    } catch { if (generation === this.metadataGeneration) this.dialogues.updateContextUsage(); }
  }
  applyAppearance() { this.host.style.setProperty('--agent-font-size', this.settings.fontSize + 'px'); }
  persist() {
    const profile = this.settings.profiles.find(entry => entry.id === this.settings.activeProfile);
    if (profile) for (const field of AGENT_PROFILE_FIELDS) profile[field] = this.settings[field];
    localPreferences.setItem(SETTINGS, JSON.stringify(this.settings));
  }
  async loadProfileKey() {
    const { activeProfile, rememberKey } = this.settings, generation = this.keyGeneration = (this.keyGeneration ?? 0) + 1;
    this.keyLoading = true; this.key = ''; this.dialogues.updateBusy();
    try {
      const key = this.memoryKeys.has(activeProfile) ? this.memoryKeys.get(activeProfile) : rememberKey ? await this.credentials.read(activeProfile) : '';
      if (generation !== this.keyGeneration) return;
      this.key = key; this.memoryKeys.set(activeProfile, key);
    } catch (error) { if (generation === this.keyGeneration) this.notify(error.message, 'warning'); }
    finally { if (generation === this.keyGeneration) { this.keyLoading = false; this.dialogues.updateBusy(); this.dialogues.updateStatus(); this.refreshCapabilities(); } }
  }
  switchProfile(id) {
    if (this.agent?.busy || this.keyLoading) return this.credentialsReady;
    const profile = this.settings.profiles.find(entry => entry.id === id); if (!profile || !this.providers.entries.has(profile.provider)) return;
    this.memoryKeys.set(this.settings.activeProfile, this.key); this.persist();
    Object.assign(this.settings, { activeProfile: id }, Object.fromEntries(AGENT_PROFILE_FIELDS.map(field => [field, profile[field]])));
    this.persist(); this.modelMetadata = null; this.dialogues.updateRouting(); this.dialogues.updateContextUsage();
    return this.credentialsReady = this.loadProfileKey();
  }
  setReasoning(effort) {
    if (this.agent?.busy || !this.reasoningOptions().includes(effort)) return;
    this.settings.reasoningEffort = effort; this.settings.thinking = effort !== 'none'; this.persist(); this.dialogues.updateRouting();
  }
  setModel(value) {
    const model = value.trim(); if (this.agent?.busy || this.keyLoading || !model || model.length > 300) { this.dialogues.updateRouting(); return; }
    this.settings.model = model; this.modelMetadata = null;
    if (!this.reasoningOptions().includes(this.settings.reasoningEffort)) { this.settings.reasoningEffort = 'auto'; this.settings.thinking = true; }
    this.persist(); this.dialogues.updateRouting(); this.dialogues.updateStatus(); this.dialogues.updateContextUsage(); this.refreshCapabilities();
  }
  open() { this.tab.hidden = false; this.activate('ai'); this.input.focus({ preventScroll: true }); }
  attach(workspace) { return this.dialogues.attach(workspace); }
  flush() { return this.dialogues.flush(); }
  async stopAndFlush() {
    this.dialogues.pauseQueue();
    if (this.agent.busy) await new Promise(resolve => {
      const finish = () => { clearTimeout(timer); this.agent.removeEventListener('state', changed); resolve(); };
      const changed = () => { if (!this.agent.busy) finish(); }, timer = setTimeout(finish, 10000);
      this.agent.addEventListener('state', changed); this.agent.stop();
    });
    await this.flush();
  }
  reset() { this.dialogues.detach(); this.agent?.reset(); for (const finish of [...this.waiting]) finish(false); this.dialogues.reset(); this.tab.hidden = true; }
  async configuration() { await this.credentialsReady; if (!this.agent?.busy) return showAgentConfiguration(this); }
}
