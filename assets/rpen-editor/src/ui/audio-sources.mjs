import { SEPARATION_MODELS } from '../core/audio-sources.mjs';
import { localPreferences } from '../platform/local-preferences.mjs';
import { pcmWave } from '../platform/pcm-wave.mjs';
import { download } from '../platform/files.mjs';
import { AudioOnsetsPanel } from './audio-onsets.mjs';
import { showAudioSeparationGuide } from './audio-separation-guide.mjs';
import { showAudioStorage, clearAudioCaches } from './audio-storage.mjs';
import { prepareAudioStorage } from '../platform/audio-storage.mjs';
import { t } from '../i18n.mjs';

const node = (tag, text = '', className = '') => { const element = document.createElement(tag); element.textContent = text; element.className = className; return element; };
export class AudioSourcesPanel {
  constructor(sources, notify = () => {}, onsets) {
    this.sources = sources; this.notify = notify; this.host = node('div', '', 'audio-sources-panel');
    try { this.options = JSON.parse(localPreferences.getItem('rpe-next-audio-separation-v1') ?? '{}'); } catch { this.options = {}; }
    if (onsets) this.onsetPanel = new AudioOnsetsPanel(onsets, sources, notify);
    sources.addEventListener('change', () => this.render()); sources.addEventListener('progress', () => this.progress()); this.render();
    sources.separator.bridge?.onProgress(value => {
      if (value.id !== 'environment' || !this.configuring) return;
      this.installLog = [...(this.installLog ?? []), value.message ?? ''].slice(-40);
      if (value.stage !== 'log' && value.stage !== 'environment' || !this.statusText) this.statusText = value.message ?? t('audio-sources.sfe06a87');
      if (this.message) this.message.textContent = this.statusText; if (this.logOutput) this.logOutput.textContent = this.installLog.join('\n');
    });
    globalThis.rpenAudioStorage?.onProgress?.(value => {
      if (value.stage !== 'cleared') return;
      this.cacheReset = this.resetAudioCaches().catch(error => this.notify(error.message, 'error'));
    });
  }
  async resetAudioCaches() {
    this.sources.controller?.abort();
    const onsets = this.onsetPanel?.onsets; onsets?.cancel(); if (onsets) { onsets.cache = new WeakMap(); onsets.clearPreview(); }
    if (this.sources.original) await this.sources.select('original');
    for (const entry of this.sources.list()) if (entry.kind === 'separated') await this.sources.remove(entry.id);
    this.statusText = t('audio-sources.sbc1189a'); this.render();
  }
  async action(task) { try { return await task(); } catch (error) { if (error.name === 'AbortError' && this.sources.selecting) return; this.statusText = error.name === 'AbortError' ? t('audio-sources.sc883ac4') : error.message; this.notify(this.statusText, error.name === 'AbortError' ? 'warning' : 'error'); if (this.message) this.message.textContent = this.statusText; } finally { this.render(); } }
  async refreshEnvironment() {
    if (!this.sources.separator.bridge || this.checkedEnvironment || this.configuring) return;
    this.checkedEnvironment = true; await this.action(async () => {
      if (!await prepareAudioStorage()) { this.checkedEnvironment = false; this.statusText = t('audio-sources.s0f010d3'); return; }
      await this.environment('probe');
    });
  }
  async environment(action) {
    if (this.configuring) return; this.configuring = true; this.installLog = []; this.statusText = action === 'probe' ? t('audio-sources.s90d3741') : t('audio-sources.s2efab3d'); this.render();
    try {
      const info = await this.sources.separator.call(action);
      if (info) { this.environmentInfo = info; this.statusText = info.ready ? t('audio-sources.sb37fb15', { p0: info.cuda ? t('audio-sources.s571404d') : 'CPU' }) : t('audio-sources.s2a60878') + info.message; }
      else this.statusText = this.environmentInfo?.ready ? t('audio-sources.s4f7e77c') : t('audio-sources.sa878516');
    }
    finally { this.configuring = false; this.render(); }
  }
  saveOptions() { localPreferences.setItem('rpe-next-audio-separation-v1', JSON.stringify(this.options)); }
  button(text, task) { const button = node('button', text); button.type = 'button'; button.onclick = () => this.action(task); return button; }
  render() {
    const sources = this.sources, entries = sources.list(), desktop = Boolean(sources.separator.bridge); this.host.replaceChildren(); this.host.append(node('h3', t('audio-sources.s5fbf022')));
    const guide = node('div', '', 'audio-source-guide');
    guide.append(node('p', desktop ? t('audio-sources.se743cbb') : t('audio-sources.seb19804'), 'hint'));
    const guideActions = node('div', '', 'audio-source-actions'); guideActions.append(this.button(t('audio-sources.s06c1e76'), () => showAudioSeparationGuide(desktop)));
    if (globalThis.rpenAudioStorage?.version === 1) {
      const clear = this.button(t('audio-sources.sa52728b'), async () => { await clearAudioCaches(this.notify); await this.cacheReset; }); clear.disabled = this.configuring || sources.busy || this.onsetPanel?.configuring || this.onsetPanel?.onsets.busy;
      guideActions.append(this.button(t('audio-sources.sef3fcff'), () => showAudioStorage(this.notify)), clear);
    }
    if (desktop) {
      this.environmentStatus = node('p', this.configuring ? t('audio-sources.s3828d0c') : this.environmentInfo?.ready ? t('audio-sources.s362e577') : this.environmentInfo ? t('audio-sources.s09a9ac5') : t('audio-sources.s1d30d4a'), 'hint'); this.environmentStatus.setAttribute('role', 'status'); guide.append(this.environmentStatus);
      const install = this.button(this.configuring ? t('audio-sources.s572335f') : this.environmentInfo?.ready ? t('audio-sources.s184acc2') : t('audio-sources.s6364be1'), () => this.environment('install')); install.disabled = this.configuring || sources.busy || this.environmentInfo?.autoInstallSupported === false; guideActions.prepend(install);
      if (this.environmentInfo?.autoInstallSupported === false) guide.append(node('p', this.environmentInfo.setup?.message ?? t('audio-sources.s734f61d'), 'hint'));
      if (this.configuring) guideActions.append(this.button(t('audio-sources.scc60165'), () => sources.separator.call('cancel', 'environment')));
    }
    guide.append(guideActions);
    const list = node('div', '', 'audio-source-list'); list.setAttribute('role', 'group'); list.setAttribute('aria-label', t('audio-sources.sc01032a'));
    const selectedCount = entries.filter(entry => entry.selected).length;
    for (const entry of entries) {
      const row = node('div', '', 'audio-source-row'), label = node('label', '', 'audio-source-choice'), checkbox = node('input'), swatch = node('span', '', 'audio-source-color'), name = node('span', entry.name, 'audio-source-name');
      checkbox.type = 'checkbox'; checkbox.checked = entry.selected; checkbox.dataset.audioSource = entry.id; checkbox.setAttribute('aria-label', entry.name); checkbox.disabled = entry.selected && selectedCount === 1; checkbox.onchange = () => this.action(() => sources.setSelected(entry.id, checkbox.checked));
      swatch.style.backgroundColor = entry.color; swatch.setAttribute('aria-hidden', 'true'); label.append(checkbox, swatch, name); row.append(label);
      if (entry.kind !== 'original') {
        const manage = node('div', '', 'audio-source-row-actions'), exportButton = this.button(t('audio-sources.s1888967'), async () => { const source = await sources.resolve(entry.id); download(await pcmWave(source.buffer), (source.stem ?? source.name.replace(/\.[^.]+$/, '')).replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_') + '.wav'); }), remove = this.button(t('audio-sources.s2f752c0'), () => sources.remove(entry.id));
        exportButton.setAttribute('aria-label', t('audio-sources.s5adb3fb') + entry.name); remove.setAttribute('aria-label', t('audio-sources.s7b379a6') + entry.name); manage.append(exportButton, remove); row.append(manage);
      }
      list.append(row);
    }
    this.host.append(list, node('p', sources.selecting ? t('audio-sources.s6fde46c') : t('audio-sources.sb5cea38'), 'hint'));
    const actions = node('div', '', 'audio-source-actions');
    actions.append(this.button(t('audio-sources.s8ca5a5b'), () => file.click())); this.host.append(actions);
    const file = node('input'); file.type = 'file'; file.accept = 'audio/*,.wav,.flac,.mp3,.ogg,.m4a'; file.multiple = true; file.hidden = true;
    file.onchange = () => this.action(async () => { for (const entry of file.files) await sources.import(await entry.arrayBuffer(), entry.name); }); this.host.append(file);
    this.host.append(node('p', t('audio-sources.s5e87e0b'), 'hint'));
    this.host.append(guide);
    const options = node('details'); options.open = this.expanded ?? false; options.ontoggle = () => { this.expanded = options.open; }; options.append(node('summary', t('audio-sources.s62f1e92')));
    const modelLabel = node('label', t('audio-sources.s1110f8d'), 'field'), model = node('select'); model.setAttribute('aria-label', t('audio-sources.s75ba4ea')); model.replaceChildren(...SEPARATION_MODELS.map(entry => new Option(entry.name, entry.id))); model.value = SEPARATION_MODELS.some(entry => entry.id === this.options.model) ? this.options.model : 'htdemucs'; model.onchange = () => { this.options.model = model.value; this.saveOptions(); }; modelLabel.append(model); options.append(modelLabel);
    const deviceLabel = node('label', t('audio-sources.s37f8f29'), 'field'), device = node('select'); device.setAttribute('aria-label', t('audio-sources.s3fbb721')); device.replaceChildren(...[['auto', t('audio-sources.s4afad87')], ['cpu', 'CPU'], ['cuda', 'NVIDIA CUDA']].map(([value, text]) => new Option(text, value))); device.value = ['auto', 'cpu', 'cuda'].includes(this.options.device) ? this.options.device : 'auto'; device.onchange = () => { this.options.device = device.value; this.saveOptions(); }; deviceLabel.append(device); options.append(deviceLabel);
    if (sources.separator.bridge) {
      const environment = node('div', '', 'audio-source-actions');
      for (const [label, action] of [[t('audio-sources.s7bdecac'), 'probe'], [t('audio-sources.sa735f51'), 'choosePython'], [t('audio-sources.sb6413a5'), 'installManual']]) { const control = this.button(label, () => this.environment(action)); control.disabled = this.configuring || sources.busy; environment.append(control); }
      options.append(environment);
      if (this.environmentInfo?.pythonPath) options.append(node('p', this.environmentInfo.pythonPath, 'hint audio-separation-status'));
    } else options.append(node('p', t('audio-sources.s391660d'), 'hint'));
    options.append(node('p', t('audio-sources.scbdcf3d'), 'hint')); this.host.append(options);
    this.message = node('p', this.statusText ?? '', 'hint audio-separation-status'); this.message.setAttribute('role', 'status'); this.host.append(this.message);
    if (this.installLog?.length) { const logs = node('details'); logs.append(node('summary', t('audio-sources.sb77a6aa'))); this.logOutput = node('pre', this.installLog.join('\n'), 'audio-separation-log'); logs.append(this.logOutput); this.host.append(logs); } else this.logOutput = null;
    const controls = node('div', '', 'audio-source-actions'); this.separateButton = this.button(sources.busy ? t('audio-sources.s45fe52c') : t('audio-sources.s0614d03'), async () => {
      if (sources.busy) { sources.controller.abort(); return; }
      if (!this.environmentInfo?.ready) { await this.environment('probe'); if (!this.environmentInfo?.ready) { this.statusText = t('audio-sources.s7ffbf7f'); return; } }
      const result = await sources.separate({ model: model.value, device: device.value }); this.statusText = t('audio-sources.s1ddf578', { p0: result.sources.length });
    }); this.separateButton.disabled = !sources.separator.bridge || !sources.original;
    this.separateButton.disabled ||= this.configuring;
    controls.append(this.separateButton); if (sources.separator.bridge) controls.append(this.button(t('audio-sources.s2bac818'), async () => { if (!confirm(t('audio-sources.s83a59c5'))) return; await sources.select('original'); await sources.separator.call('clear'); for (const entry of sources.list()) if (entry.kind === 'separated') await sources.remove(entry.id); this.notify(t('audio-sources.sd1bfc3d'), 'success'); })); this.host.append(controls); this.progress();
  }
  progress() {
    const value = this.sources.progress; if (!value || !this.message) return;
    this.message.textContent = value.stage === 'separating' ? t('audio-sources.s8387b47', { p0: value.frames > 0 ? t('audio-sources.s4022d40', { p0: (value.segment / value.frames * 100).toFixed(0) }) : '' }) : value.message || t('audio-sources.s9ca319d');
  }
}
