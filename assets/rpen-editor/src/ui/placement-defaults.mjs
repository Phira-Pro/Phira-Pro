import { normalizeNoteDefaults, normalizeLineDefaults } from '../core/placement-defaults.mjs';
import { parseNoteColor } from '../core/note-properties.mjs';
import { AUDIO_EXTENSION } from '../core/asset-library.mjs';
import { assetPath } from '../platform/files.mjs';
import { assetReference } from '../platform/asset-library.mjs';
import { NotePropertyControls } from './note-property-controls.mjs';
import { numericWheel } from './numeric-wheel.mjs';
import { t } from '../i18n.mjs';

export class PlacementDefaultsPanel {
  constructor(getContext, { writeLineDefaults, invalidate, reportError }) {
    Object.assign(this, { getContext, writeLineDefaults, invalidate, reportError }); this.tab = 'note';
    this.panel = document.createElement('div'); this.panel.id = 'placement-defaults-panel'; this.panel.className = 'placement-defaults-panel'; this.panel.hidden = true;
    this.panel.setAttribute('role', 'dialog'); document.body.append(this.panel);
    window.addEventListener('pointerdown', event => { if (!this.panel.hidden && !this.panel.contains(event.target)) this.hide(); }, true);
    window.addEventListener('keydown', event => { if (!this.panel.hidden && event.key === 'Escape') { this.hide(); event.preventDefault(); event.stopImmediatePropagation(); } }, true);
    this.panel.addEventListener('keydown', event => event.stopPropagation());
    window.addEventListener('resize', () => { if (!this.panel.hidden) this.position(); });
    this.resizeObserver = new ResizeObserver(() => { if (!this.panel.hidden) this.position(); }); this.resizeObserver.observe(this.panel);
  }
  open(x, y) { this.owner = this.getContext().session; this.x = x; this.y = y; this.render(); this.panel.hidden = false; this.position(); this.tabs.get(this.tab).focus({ preventScroll: true }); }
  hide() { this.panel.hidden = true; this.controls?.dispose(); this.controls = null; }
  position() {
    const bounds = this.panel.getBoundingClientRect();
    this.panel.style.left = `${Math.max(4, Math.min(innerWidth - bounds.width - 4, this.x))}px`;
    this.panel.style.top = `${Math.max(4, Math.min(innerHeight - bounds.height - 4, this.y))}px`;
  }
  get values() { return this.tab === 'note' ? normalizeNoteDefaults(this.owner.noteDefaults) : normalizeLineDefaults(this.owner.lineEventDefaults); }
  write(values) {
    if (this.owner !== this.getContext().session) { this.hide(); return; }
    if (this.tab === 'note') this.owner.noteDefaults = normalizeNoteDefaults(values);
    else this.writeLineDefaults(normalizeLineDefaults(values));
    this.invalidate();
  }
  render() {
    this.controls?.dispose(); this.panel.replaceChildren(); this.panel.setAttribute('aria-label', t('placement-defaults.title'));
    const heading = document.createElement('div'); heading.className = 'placement-defaults-heading';
    const title = document.createElement('strong'); title.textContent = t('placement-defaults.title');
    const close = document.createElement('button'); close.type = 'button'; close.textContent = '×'; close.setAttribute('aria-label', t('selection-interval.s60c5d87')); close.onclick = () => this.hide(); heading.append(title, close);
    const tabs = document.createElement('div'); tabs.className = 'placement-defaults-tabs'; tabs.setAttribute('role', 'tablist'); tabs.setAttribute('aria-label', title.textContent); this.tabs = new Map();
    for (const [key, text] of [['note', t('placement-defaults.notes')], ['line', t('transport.line')]]) {
      const button = document.createElement('button'); button.type = 'button'; button.textContent = text; button.id = `placement-defaults-${key}-tab`; button.dataset.defaultsTab = key;
      button.setAttribute('role', 'tab'); button.setAttribute('aria-selected', String(key === this.tab)); button.setAttribute('aria-controls', 'placement-defaults-body'); button.tabIndex = key === this.tab ? 0 : -1;
      button.onclick = () => { this.tab = key; this.render(); this.position(); this.tabs.get(key).focus({ preventScroll: true }); };
      button.onkeydown = event => { if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) { event.preventDefault(); this.tabs.get(event.key === 'Home' ? 'note' : event.key === 'End' ? 'line' : key === 'note' ? 'line' : 'note').click(); } };
      tabs.append(button); this.tabs.set(key, button);
    }
    this.body = document.createElement('div'); this.body.className = 'placement-defaults-body'; this.body.id = 'placement-defaults-body'; this.body.setAttribute('role', 'tabpanel'); this.body.setAttribute('aria-labelledby', `placement-defaults-${this.tab}-tab`);
    const footer = document.createElement('div'); footer.className = 'placement-defaults-footer';
    const reset = document.createElement('button'); reset.type = 'button'; reset.className = 'placement-defaults-reset'; reset.textContent = t('placement-defaults.reset'); reset.onclick = () => { this.write(this.tab === 'note' ? normalizeNoteDefaults() : normalizeLineDefaults()); this.render(); this.position(); }; footer.append(reset);
    this.panel.append(heading, tabs, this.body, footer);
    this.controls = new NotePropertyControls({}, this.body, { volume: () => this.getContext().volume, reportError: this.reportError });
    if (this.tab === 'note') this.noteFields(); else this.lineFields();
  }
  field(key, text, { kind = 'number', choices, step = 1, min, max } = {}) {
    const label = document.createElement('label'); label.className = 'field'; label.dataset.rpenField = key; label.append(text);
    const input = document.createElement(choices ? 'select' : 'input'); input.dataset.rpenField = key; input.dataset.defaultProperty = key; input.setAttribute('aria-label', text);
    if (choices) input.replaceChildren(...choices.map(([value, name]) => new Option(name, value)));
    else { input.type = kind === 'number' ? 'number' : 'text'; input.step = step; if (min !== undefined) input.min = min; if (max !== undefined) input.max = max; }
    const value = this.values[key]; input.value = Array.isArray(value) ? value.join(', ') : value;
    let colorControl;
    const apply = () => {
      try {
        let value;
        if (kind === 'color') value = parseNoteColor(input.value);
        else if (kind === 'sound') {
          const { assets, chartName } = this.getContext(), source = input.value.trim(), path = source ? assetPath(assets, source, chartName) : '';
          if (source && (!path || !AUDIO_EXTENSION.test(path))) throw Error(t('note-properties.invalidSound'));
          value = path ? assetReference(assets, path, chartName) : '';
        } else {
          value = Number(input.value);
          if (!input.value.trim() || !Number.isFinite(value) || min !== undefined && value < min || max !== undefined && value > max) throw Error(t('placement-defaults.invalid'));
        }
        this.write({ ...this.values, [key]: value }); input.setCustomValidity(''); input.removeAttribute('aria-invalid'); colorControl?.sync(this.values[key]);
      } catch (error) { input.setCustomValidity(error.message); input.setAttribute('aria-invalid', 'true'); }
    };
    input.oninput = event => { if (!event.isComposing) apply(); }; input.onchange = apply;
    if (kind === 'color') colorControl = this.controls.color({ label, input, key, title: text, value, apply });
    else label.append(input);
    this.body.append(label);
    if (kind === 'number' && !choices) numericWheel(input, step);
    if (kind === 'sound') {
      const { assets, chartName, session } = this.getContext();
      this.controls.sounds({ assets, chart: session.chart, chartName, input, onToggle: () => {}, apply });
    }
  }
  noteFields() {
    this.field('above', t('inspector.sa88d531'), { choices: [[1, t('inspector.sc847d66')], [2, t('inspector.se1dea69')]] });
    this.field('isFake', 'Fake', { choices: [[0, t('inspector.s8bf5c10')], [1, t('inspector.s30160a2')]] });
    this.field('speed', t('inspector.sf2fdffb'), { step: .1 });
    this.field('size', t('inspector.sfd20702'), { step: .25, min: .01 });
    this.field('judgeArea', t('note-properties.judgeArea'), { step: .25 });
    this.field('alpha', t('inspector.s61b101b'), { min: 0, max: 255 });
    this.field('yOffset', t('inspector.s9d63ee8'), { step: 5 });
    this.field('visibleTime', t('placement-defaults.visibleTime'), { min: 0 });
    this.field('tint', t('note-properties.color'), { kind: 'color' });
    this.field('tintHitEffects', t('note-properties.hitColor'), { kind: 'color' });
    this.field('hitsound', t('note-properties.hitSound'), { kind: 'sound' });
  }
  lineFields() {
    for (const [key, title] of [['moveXEvents', 'X'], ['moveYEvents', 'Y'], ['rotateEvents', t('timeline.sa69e51e')], ['alphaEvents', t('inspector.s61b101b')], ['speedEvents', t('inspector.sf2fdffb')]]) this.field(key, title, { step: key === 'speedEvents' ? .1 : 1, ...(key === 'alphaEvents' ? { max: 255 } : {}) });
  }
}
