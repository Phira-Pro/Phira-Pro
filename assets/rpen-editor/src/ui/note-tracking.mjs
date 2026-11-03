import { NoiseTrackingPanel } from './noise-tracking.mjs';
import { trackedNoteEvents, noteTrackingChart } from '../application/note-tracking.mjs';
import { lineDisplayLabel } from '../core/line-groups.mjs';
import { parseBeat, formatBeat, fromNumber } from '../core/beat.mjs';
import { t } from '../i18n.mjs';

const button = (text, action) => { const node = document.createElement('button'); node.type = 'button'; node.textContent = text; node.onclick = action; return node; };

export class NoteTrackingPanel extends NoiseTrackingPanel {
  constructor(...args) {
    super(...args); Object.assign(this.options, { mode: 'note', lineIndex: -1, layer: 0, rotation: true, replace: false });
    this.render(); this.sampler.classList.add('note-tracking-sampler');
  }
  get target() { return this.noteTarget; }
  open() {
    this.pause(); const { session, timeline, tempo, seconds } = this.context;
    if (!this.initialized || this.owner !== session) {
      this.options.lineIndex = session.lineIndex; this.options.layer = timeline.layer;
      const beat = tempo.beat(seconds()); this.options.start = formatBeat(fromNumber(beat)); this.options.end = formatBeat(fromNumber(beat + 4));
      this.previewSeconds = seconds(); this.initialized = true;
      const selected = session.selectedNoteEntries();
      if (selected.length === 1) { const source = selected[0]; this.noteTarget = { mode: 'note', lineIndex: source.lineIndex, noteIndex: source.index }; this.noteReference = source.note; }
    }
    this.owner = session; this.active = true; this.baseChart = null; this.activate('note-tracking'); this.render(); this.sync();
  }
  render() {
    this.host.replaceChildren(); this.controls = new Map(); this.scroll = document.createElement('div'); this.scroll.className = 'noise-tracking-scroll'; this.host.append(this.scroll);
    const title = document.createElement('div'); title.className = 'panel-title'; title.textContent = t('note-track.title'); this.scroll.append(title);
    this.targetLabel = document.createElement('p'); this.targetLabel.className = 'hint'; this.scroll.append(this.targetLabel);
    this.sampleButton = button(t('note-track.sample'), () => this.beginSample()); this.sampleButton.className = 'wide-button'; this.scroll.append(this.sampleButton);
    this.field('start', t('noise-tracking.s087c451'), { beat: true }); this.field('end', t('noise-tracking.sc4d3fa4'), { beat: true });
    this.field('lineIndex', t('note-track.destination'), { choices: this.lines() });
    this.field('layer', t('note-track.layer'), { choices: [0, 1, 2, 3].map(index => [index, String(index)]) });
    this.field('rotation', t('note-track.rotation'), { checkbox: true }); this.field('replace', t('note-track.replace'), { checkbox: true });
    this.field('sampleRate', t('noise-tracking.s423c4be'), { choices: [[60, '60 Hz'], [120, '120 Hz'], [240, '240 Hz']] });
    this.field('show', t('noise-tracking.sf43e6d2'), { checkbox: true });
    const playback = document.createElement('div'); playback.className = 'noise-tracking-playback';
    this.playButton = button(t('noise-tracking.see93fc2'), () => { this.animating = !this.animating; this.animationEpoch = null; this.playButton.textContent = t(this.animating ? 'noise-tracking.s41852e3' : 'noise-tracking.see93fc2'); this.invalidate(); });
    this.progress = document.createElement('input'); this.progress.type = 'range'; this.progress.min = 0; this.progress.max = 1; this.progress.step = .001; this.progress.value = 0; this.progress.setAttribute('aria-label', t('noise-tracking.sbf11602'));
    this.progress.oninput = () => { this.animating = false; this.playButton.textContent = t('noise-tracking.see93fc2'); if (this.range) this.previewSeconds = this.range.start + Number(this.progress.value) * (this.range.end - this.range.start); this.invalidate(); };
    this.previewTime = document.createElement('output'); playback.append(this.playButton, this.progress, this.previewTime); this.scroll.append(playback);
    const hint = document.createElement('p'); hint.className = 'hint'; hint.textContent = t('note-track.help'); this.scroll.append(hint);
    const footer = document.createElement('div'); footer.className = 'noise-tracking-footer'; this.message = document.createElement('p'); this.message.className = 'hint'; this.message.setAttribute('role', 'status');
    this.generate = button(t('note-track.generate'), () => this.apply()); this.generate.className = 'primary'; this.generate.disabled = true;
    footer.append(this.message, button(t('noise-tracking.sb3669b2'), () => this.activate('chart')), this.generate); this.host.append(footer); this.updateVisibility();
  }
  lines() { return [[-1, t('note-track.newLine')], ...this.context.session.chart.judgeLineList.map((line, index) => [index, lineDisplayLabel(this.context.session.chart, index)])]; }
  updateVisibility() { if (this.targetLabel) this.targetLabel.textContent = this.noteTarget ? t('noise-tracking.s8dfdc65', { p0: lineDisplayLabel(this.context.session.chart, this.noteTarget.lineIndex), p1: this.noteTarget.noteIndex }) : t('note-track.select'); }
  rebuild() {
    if (!this.active) return;
    const { session, tempo } = this.context; this.baseChart = session.chart; this.baseTempo = tempo;
    try {
      const start = parseBeat(this.options.start), end = parseBeat(this.options.end);
      this.range = { start: tempo.seconds(start), end: tempo.seconds(end) };
      this.generated = trackedNoteEvents(session.chart, tempo, { ...this.options, start, end, lineIndex: Number(this.options.lineIndex), layer: Number(this.options.layer), sampleRate: Number(this.options.sampleRate), target: this.target, lineDefaults: session.lineEventDefaults });
      let conflict;
      try { this.previewChart = this.appliedArea === session.chart ? session.chart : noteTrackingChart(session.chart, this.generated, this.options.replace, tempo); }
      catch (error) {
        if (error.code !== 'NOTE_TRACK_CONFLICT') throw error;
        // Picking a source remains successful even when its destination needs explicit replacement.
        this.previewChart = noteTrackingChart(session.chart, this.generated, true, tempo); conflict = error;
      }
      this.area = conflict ? null : this.generated;
      this.previewSeconds = Math.max(this.range.start, Math.min(this.range.end - 1e-7, this.previewSeconds ?? this.range.start));
      this.message.classList.toggle('error', Boolean(conflict)); this.message.textContent = conflict ? t('note-track.pickedConflict') : t('note-track.count', { count: [...this.generated.tracks.values()].reduce((sum, events) => sum + events.length, 0) });
    } catch (error) { this.fail(error.message); return; }
    this.updateChrome(); this.invalidate();
  }
  sync() {
    if (!this.active) return; if (this.owner !== this.context.session) { this.reset(); return; }
    if (this.baseChart === this.context.session.chart && this.baseTempo === this.context.tempo) return;
    if (this.noteTarget) {
      const lines = this.context.session.chart.judgeLineList, lineIndex = lines.findIndex(line => line.notes?.includes(this.noteReference));
      if (lineIndex < 0) { this.noteTarget = null; this.noteReference = null; }
      else this.noteTarget = { mode: 'note', lineIndex, noteIndex: lines[lineIndex].notes.indexOf(this.noteReference) };
    }
    if (Number(this.options.lineIndex) >= 0 && !this.context.session.chart.judgeLineList[Number(this.options.lineIndex)]) this.options.lineIndex = this.context.session.lineIndex;
    const input = this.controls.get('lineIndex'); input.replaceChildren(...this.lines().map(([value, text]) => new Option(text, value))); input.value = this.options.lineIndex;
    this.updateVisibility(); this.rebuild();
  }
  apply() {
    try {
      this.sync(); if (!this.area || !this.previewChart || this.sampling) throw Error(this.generated ? this.message.textContent : t('note-track.select'));
      const { session } = this.context, chart = noteTrackingChart(session.chart, this.generated, this.options.replace, this.context.tempo);
      this.appliedArea = chart; session.commit(t('note-track.title'), chart); this.rebuild(); this.notify(t('note-track.done'), 'success');
    } catch (error) { this.fail(error.message); this.notify(error.message, 'error'); }
  }
}
