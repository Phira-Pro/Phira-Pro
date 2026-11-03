import { ONSET_DETECTORS } from '../core/audio-onsets.mjs';
import { localPreferences } from '../platform/local-preferences.mjs';
import { MAPPER_MODELS } from '../core/audio-mapper.mjs';
import { t } from '../i18n.mjs';

const node = (tag, text = '', className = '') => { const element = document.createElement(tag); element.textContent = text; element.className = className; return element; };
const preferenceKey = 'rpe-next-audio-onsets-v1';

export class AudioOnsetsPanel {
  constructor(onsets, sources, notify) {
    this.onsets = onsets; this.sources = sources; this.notify = notify; this.host = node('div', '', 'audio-onsets-panel');
    try { const options = JSON.parse(localPreferences.getItem(preferenceKey) ?? '{}'); if (options.detector === 'mapper' && !onsets.mapper.bridge || options.detector === 'ddc' && !onsets.detector.bridge) options.detector = 'flux'; onsets.configure(options); } catch {}
    onsets.addEventListener('change', () => this.render()); onsets.addEventListener('progress', () => this.progress());
    for (const backend of [onsets.detector, onsets.mapper]) backend.bridge?.onProgress(value => { if (value.id === 'environment' && this.configuring) { this.statusText = value.message ?? t('audio-onsets.sd7b86bc'); this.progress(); } }); this.render();
  }
  button(label, task) { const button = node('button', label); button.type = 'button'; button.onclick = async () => { try { await task(); } catch (error) { this.statusText = error.message; this.notify(error.message, error.name === 'AbortError' ? 'warning' : 'error'); } this.render(); }; return button; }
  async environment(action) {
    if (this.configuring) return; this.configuring = true; this.statusText = t('audio-onsets.sec8954c'); this.render();
    try { const backend = this.onsets.options.detector === 'mapper' ? this.onsets.mapper : this.onsets.detector, info = await backend.call(action); if (info) this.statusText = info.ready ? t('audio-onsets.s5bf1ec3', { p0: info.engine === 'mapper' ? 'Mapperatorinator · PyTorch ' + info.torch : 'DDC ' + info.version, p1: info.cuda ? t('audio-onsets.scdfcf6d') : 'CPU', p2: info.pythonPath }) : info.message; }
    finally { this.configuring = false; this.render(); }
  }
  setting(name, value) { this.onsets.configure({ [name]: value }); localPreferences.setItem(preferenceKey, JSON.stringify(this.onsets.options)); }
  async analyze() {
    if (this.onsets.busy) { this.onsets.cancel(); return; }
    const entry = await this.sources.resolveForAnalysis(), revision = this.sources.revision, offset = 0, selection = this.sources.selectedIds.join('\0');
    const current = Number(this.sources.getAudio().time) || 0, end = Math.min(offset + entry.buffer.duration, current + 10);
    if (!(end > current)) throw Error(t('audio-onsets.sfe4fd1b'));
    const range = this.sources.range(entry, current, end), options = this.onsets.options;
    const result = await this.onsets.analyze(entry.buffer, { ...range, sourceOffset: offset });
    if (revision !== this.sources.revision || !this.sources.isCurrent(entry)) throw Error(t('audio-onsets.s5655475'));
    this.onsets.preview = { ...result, buffer: entry.buffer, source: entry.id, selection, sourceRevision: revision, offsetSeconds: 0 }; this.onsets.changed();
    this.statusText = t('audio-onsets.s4502cb0', { p0: result.peaks.length, p1: options.detector === 'mapper' ? t('audio-onsets.scfa5beb') : t('audio-onsets.s6597711'), p2: current.toFixed(2), p3: end.toFixed(2), p4: options.detector === 'mapper' ? 'Mapperatorinator ' + result.model.name : options.detector === 'ddc' ? 'DDC' : t('audio-onsets.s97b3dd0') }); this.render();
  }
  render() {
    const onsets = this.onsets, isMapper = onsets.options.detector === 'mapper', backend = isMapper ? onsets.mapper : onsets.detector; this.host.replaceChildren(); this.host.append(node('h3', t('audio-onsets.s239ca8d')));
    const algorithm = node('label', t('audio-onsets.s2b362f8'), 'field'), select = node('select'); select.setAttribute('aria-label', t('audio-onsets.s7966d12')); select.replaceChildren(...ONSET_DETECTORS.map(entry => new Option(entry.name, entry.id))); select.value = onsets.options.detector; select.disabled = onsets.busy || this.configuring; select.onchange = () => this.setting('detector', select.value); algorithm.append(select); this.host.append(algorithm);
    if (isMapper) {
      for (const [name, title, choices] of [['mapperModel', t('audio-onsets.s0ad017e'), MAPPER_MODELS.map(model => [model.id, model.name])], ['mapperMode', t('audio-onsets.s85dcb29'), [[3, t('audio-onsets.sd3f29cb')], [0, t('audio-onsets.s5e1d94c')]]]]) {
        const label = node('label', title, 'field'), input = node('select'); input.setAttribute('aria-label', title); input.replaceChildren(...choices.map(([value, text]) => new Option(text, value))); input.value = onsets.options[name]; input.disabled = onsets.busy || this.configuring; input.onchange = () => this.setting(name, name === 'mapperMode' ? Number(input.value) : input.value); label.append(input); this.host.append(label);
      }
      for (const [name, title, min, max, step] of [['difficulty', t('audio-onsets.s9f6b3c7'), .1, 15, .1], ['seed', t('audio-onsets.sed73926'), 0, 0xffffffff, 1]]) {
        const label = node('label', title, 'field'), input = node('input'); input.type = 'number'; input.min = min; input.max = max; input.step = step; input.value = onsets.options[name]; input.setAttribute('aria-label', title); input.disabled = onsets.busy; input.onchange = () => { try { this.setting(name, Number(input.value)); } catch (error) { this.notify(error.message, 'error'); this.render(); } }; label.append(input); this.host.append(label);
      }
      const label = node('label', t('audio-onsets.sb1ad39a'), 'field'), checkbox = node('input'); checkbox.type = 'checkbox'; checkbox.checked = onsets.options.useChartTiming; checkbox.disabled = onsets.busy; checkbox.setAttribute('aria-label', t('audio-onsets.sb1ad39a')); checkbox.onchange = () => this.setting('useChartTiming', checkbox.checked); label.append(checkbox); this.host.append(label);
    }
    for (const [name, text, min, max, step] of [['minConfidence', t('audio-onsets.s627216b'), 0, 1, .01], ['minGap', t('audio-onsets.s2557401'), .02, 2, .01]]) {
      if (isMapper && name === 'minConfidence') continue;
      const label = node('label', text, 'field'), input = node('input'); input.type = 'number'; input.min = min; input.max = max; input.step = step; input.value = onsets.options[name]; input.setAttribute('aria-label', text); input.disabled = onsets.busy; input.onchange = () => { try { this.setting(name, Number(input.value)); } catch (error) { this.notify(error.message, 'error'); this.render(); } }; label.append(input); this.host.append(label);
    }
    this.host.append(node('p', isMapper ? t('audio-onsets.sff05dc5') : t('audio-onsets.s0ec64b2'), 'hint'));
    const environment = node('details'); environment.open = this.expanded ?? false; environment.ontoggle = () => { this.expanded = environment.open; }; environment.append(node('summary', isMapper ? t('audio-onsets.saffa87a') : t('audio-onsets.s1f67372')));
    if (backend.bridge) {
      const controls = node('div', '', 'audio-source-actions');
      for (const [label, action] of [[t('audio-onsets.s5321f48'), 'probe'], [t('audio-onsets.sa735f51'), 'choosePython'], [t('audio-onsets.s3a03c22'), 'install']]) { const button = this.button(label, () => this.environment(action)); button.disabled = onsets.busy || this.configuring; controls.append(button); }
      if (this.configuring) controls.append(this.button(t('audio-onsets.saa385c0'), () => backend.call('cancel', 'environment'))); environment.append(controls);
      const deviceLabel = node('label', t('audio-onsets.s37f8f29'), 'field'), device = node('select'); device.setAttribute('aria-label', t('audio-onsets.sb98291d')); device.replaceChildren(...[['auto', t('audio-onsets.s4afad87')], ['cpu', 'CPU'], ['cuda', 'NVIDIA CUDA']].map(([value, label]) => new Option(label, value))); device.value = onsets.options.device; device.disabled = onsets.busy; device.onchange = () => this.setting('device', device.value); deviceLabel.append(device); environment.append(deviceLabel);
      environment.append(this.button(t('audio-onsets.s3966b13'), async () => { if (!confirm(t('audio-onsets.sbba10e9'))) return; await backend.call('clear'); onsets.cache = new WeakMap(); onsets.clearPreview(); this.statusText = t('audio-onsets.s8418be9'); }));
    } else environment.append(node('p', t('audio-onsets.s51b01d2'), 'hint'));
    this.host.append(environment); const controls = node('div', '', 'audio-source-actions'), analyze = this.button(onsets.busy ? t('audio-onsets.sed40b99') : t('audio-onsets.s0ddbe76'), () => this.analyze()); analyze.disabled = this.configuring || !this.sources.original; controls.append(analyze, this.button(t('audio-onsets.s143d8ea'), () => { onsets.clearPreview(); this.statusText = ''; })); this.host.append(controls);
    this.message = node('p', '', 'hint audio-onsets-status'); this.message.setAttribute('role', 'status'); this.host.append(this.message); this.progress();
  }
  progress() { if (this.message) this.message.textContent = this.onsets.progress?.message ?? this.statusText ?? ''; }
}
