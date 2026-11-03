import { libraryAudio } from '../core/asset-library.mjs';
import { assetPath } from '../platform/files.mjs';
import { assetReference } from '../platform/asset-library.mjs';
import { AssetAudioPlayer } from './asset-audio.mjs';
import { parseNoteColor } from '../core/note-properties.mjs';
import { t } from '../i18n.mjs';

export class NotePropertyControls {
  constructor(session, container, { volume, reportError } = {}) {
    this.session = session; this.container = container; this.controller = new AbortController();
    this.audio = new AssetAudioPlayer({ volume, reportError });
    document.addEventListener('pointerdown', event => { if (this.active && !container.contains(event.target)) this.close(); }, { capture: true, signal: this.controller.signal });
  }
  close() {
    if (this.active) { this.active.panel.hidden = true; this.active.button.setAttribute('aria-expanded', 'false'); }
    this.active = null; this.session.liveNoteEdit = false;
  }
  dispose() { this.close(); this.audio.stop(); this.controller.abort(); }
  color({ label, input, key, title, value, apply }) {
    const panel = document.createElement('div'); panel.className = 'event-sample-panel note-color-sample'; panel.dataset.rpenField = key; panel.hidden = true;
    const picker = document.createElement('input'); picker.type = 'color'; picker.setAttribute('aria-label', t('note-properties.colorPicker', { title })); panel.append(picker);
    let color = [...value]; const channels = [];
    const sync = next => {
      color = [...next]; input.value = color.join(', '); picker.value = '#' + color.map(channel => channel.toString(16).padStart(2, '0')).join('');
      channels.forEach(({ range, output }, index) => { range.value = color[index]; output.textContent = color[index]; });
    };
    const update = next => { this.session.liveNoteEdit = true; sync(next); apply(next); };
    picker.oninput = () => update([0, 2, 4].map(index => Number.parseInt(picker.value.slice(index + 1, index + 3), 16)));
    for (const [index, name] of ['R', 'G', 'B'].entries()) {
      const row = document.createElement('label'); row.className = 'field'; row.append(name);
      const range = document.createElement('input'); range.type = 'range'; range.min = 0; range.max = 255; range.step = 1; range.setAttribute('aria-label', title + ' ' + name);
      const output = document.createElement('output'); channels.push({ range, output });
      range.oninput = () => { const next = [...color]; next[index] = Number(range.value); update(next); };
      row.append(range, output); panel.append(row);
    }
    const close = document.createElement('button'); close.type = 'button'; close.textContent = t('note-properties.close'); close.onclick = () => this.close(); panel.append(close);
    const controls = document.createElement('span'); controls.className = 'event-input-controls';
    const button = document.createElement('button'); button.type = 'button'; button.className = 'sample-button'; button.textContent = '⌖'; button.title = t('note-properties.colorPicker', { title }); button.setAttribute('aria-label', button.title); button.setAttribute('aria-expanded', 'false');
    button.onpointerdown = event => event.preventDefault();
    button.onclick = () => { if (this.active?.panel === panel) { this.close(); return; } this.close(); let next; try { next = parseNoteColor(input.value); } catch { next = color; } sync(next); this.active = { panel, button }; panel.hidden = false; button.setAttribute('aria-expanded', 'true'); this.session.liveNoteEdit = true; };
    sync(value); controls.append(input, button); label.append(controls); this.container.append(panel);
    return { sync };
  }
  sounds({ assets, chart, chartName, input, open = false, onToggle, apply }) {
    const details = document.createElement('details'); details.className = 'note-sound-library'; details.open = open;
    details.addEventListener('toggle', () => onToggle(details.open));
    const summary = document.createElement('summary'); summary.textContent = t('note-properties.chooseSound'); details.append(summary);
    const list = document.createElement('div'); list.className = 'note-sound-list'; const entries = libraryAudio(assets, chart, chartName);
    const buttons = new Map(), sync = reference => {
      const selected = assetPath(assets, reference, chartName) ?? reference;
      for (const [path, button] of buttons) button.classList.toggle('selected', path === (selected || ''));
    };
    for (const [path, bytes] of [['', null], ...entries]) {
      const row = document.createElement('div'); row.className = 'note-sound-row';
      const choose = document.createElement('button'); choose.type = 'button'; choose.className = 'note-sound-select'; choose.dataset.hitsound = path; choose.textContent = path || t('note-properties.defaultSound'); choose.title = choose.textContent;
      choose.onpointerdown = event => event.preventDefault();
      choose.onclick = () => {
        input.value = path ? assetReference(assets, path, chartName) : ''; this.session.liveNoteEdit = true;
        try { apply(); sync(input.value); } finally { this.session.liveNoteEdit = Boolean(this.active); }
      };
      buttons.set(path, choose); row.append(choose);
      if (bytes) { const play = document.createElement('button'); play.type = 'button'; play.className = 'note-sound-play'; play.textContent = '▶'; play.title = t('asset-audio.preview', { name: path }); play.setAttribute('aria-label', play.title); play.onpointerdown = event => event.preventDefault(); play.onclick = () => this.audio.play(bytes, path); row.append(play); }
      list.append(row);
    }
    details.append(list, this.audio.element); sync(input.value);
    if (!entries.length) { const hint = document.createElement('p'); hint.className = 'hint'; hint.textContent = t('note-properties.noSounds'); details.append(hint); }
    this.container.append(details);
  }
}
