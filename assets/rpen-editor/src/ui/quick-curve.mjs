import { beatValue, fromNumber } from '../core/beat.mjs';
import { quickCurveNotes, quickCurveChart } from '../core/quick-curve.mjs';
import { usesNativeTextShortcut } from './keyboard.mjs';
import { t } from '../i18n.mjs';

export class QuickCurve {
  constructor(timeline, getContext, { pause, invalidate, notify }) {
    Object.assign(this, { timeline, getContext, pause, invalidate, notify }); this.density = 1;
    timeline.notesCanvas.addEventListener('pointerdown', event => {
      this.cancel(); if (event.button !== 1 || event.shiftKey || event.ctrlKey || event.altKey || timeline.pendingHold || timeline.eventInteraction.pending || !getContext().enabled) return;
      const session = getContext().session, selected = session.selectedNoteEntries(); if (selected.length !== 1) return;
      const source = selected[0]; this.pending = { ...source, session, chart: session.chart, pointerId: event.pointerId, point: timeline.point(event) };
      this.timer = setTimeout(() => this.begin(), 750);
    }, true);
    window.addEventListener('pointermove', event => {
      if (this.pending?.pointerId !== event.pointerId) return;
      this.pending.point = timeline.point(event); if (this.active) { event.preventDefault(); this.update(); }
    }, true);
    window.addEventListener('pointerup', event => {
      if (this.pending?.pointerId !== event.pointerId) return;
      clearTimeout(this.timer);
      if (this.active) { event.preventDefault(); event.stopImmediatePropagation(); this.pending.point = timeline.point(event); this.update(); this.apply(); }
      else this.pending = null;
    }, true);
    window.addEventListener('pointercancel', () => this.cancel()); window.addEventListener('blur', () => this.cancel());
    window.addEventListener('keydown', event => {
      if (!this.active || usesNativeTextShortcut(event)) return;
      event.preventDefault(); event.stopImmediatePropagation(); if (event.repeat) return;
      if (event.key === 'Escape') { this.cancel(); return; }
      if (/^[0-9]$/.test(event.key)) {
        const digit = Number(event.key), now = performance.now();
        if (this.firstDigit && now - this.digitTime < 2000) { const easing = this.firstDigit * 10 + digit; if (easing <= 29) this.easingType = easing; this.firstDigit = 0; }
        else if (digit) { this.easingType = digit; this.firstDigit = digit <= 2 ? digit : 0; this.digitTime = now; }
      } else if (/^[qwer]$/i.test(event.key)) { this.density = 'qwer'.indexOf(event.key.toLowerCase()) + 1; this.firstDigit = 0; }
      this.update();
    }, true);
  }
  begin() {
    const pending = this.pending, context = this.getContext();
    const selected = context.session.selectedNoteEntries();
    if (!pending || !context.enabled || pending.session !== context.session || pending.chart !== context.session.chart || selected.length !== 1 || selected[0].note !== pending.note) return this.cancel();
    this.pause(); this.timeline.cancelPlacement(); this.active = true; this.easingType = 1; this.firstDigit = 0; this.key = null;
    if (!this.explained) { this.notify(t('quick-curve.help'), 'info', 6000); this.explained = true; }
    this.update();
  }
  update() {
    const pending = this.pending; if (!this.active || !pending) return;
    const { session, tempo, enabled } = this.getContext(); if (!enabled || session !== pending.session || session.chart !== pending.chart) return this.cancel();
    const target = this.timeline.lineIndexAt(pending.point.x, this.timeline.notesCanvas.clientWidth), x = this.timeline.notePositionAt(pending.point.x, target);
    const factor = session.chart.judgeLineList[target].bpmfactor ?? 1;
    const end = this.timeline.noteView?.flow ? this.timeline.snappedBeat(pending.point.y) : this.timeline.tempo.beat(this.timeline.timeAt(pending.point.y), factor);
    const endBeat = fromNumber(Math.max(0, Math.round(end * this.timeline.division) / this.timeline.division));
    const sourceTime = tempo.seconds(pending.note.startTime, session.chart.judgeLineList[pending.lineIndex].bpmfactor ?? 1);
    const startNote = { ...pending.note, startTime: fromNumber(tempo.beat(sourceTime, factor)) };
    const key = [session.chart, target, x, beatValue(endBeat), this.easingType, this.density, this.timeline.division];
    if (this.key && key.every((value, index) => value === this.key[index])) return;
    this.key = key;
    try {
      const notes = quickCurveNotes(startNote, endBeat, x, { division: this.timeline.division, density: this.density, easingType: this.easingType, noteDefaults: session.noteDefaults });
      const sourceFactor = session.chart.judgeLineList[pending.lineIndex].bpmfactor ?? 1;
      this.notes = notes.map((note, index) => index === notes.length - 1 || target === pending.lineIndex ? note : { ...note, startTime: fromNumber(tempo.beat(tempo.seconds(note.startTime, factor), sourceFactor)), endTime: fromNumber(tempo.beat(tempo.seconds(note.endTime, factor), sourceFactor)) });
      this.target = target; this.chart = quickCurveChart(session.chart, pending.lineIndex, this.notes, target);
      this.ghost = this.notes.map((note, index) => ({ ...note, lineIndex: index === this.notes.length - 1 ? target : pending.lineIndex }));
      this.timeline.quickCurveGhost = this.ghost; this.timeline.quickCurveLabel = t('quick-curve.status', { easing: this.easingType, density: this.density, count: notes.length });
    } catch (error) { this.chart = null; this.notes = []; this.timeline.quickCurveGhost = []; this.timeline.quickCurveLabel = error.message; }
    this.invalidate();
  }
  view() { if (!this.active) return null; this.update(); return this.chart; }
  apply() {
    const { session } = this.getContext(), pending = this.pending, chart = this.chart, count = this.notes?.length ?? 0;
    this.cancel(); if (pending?.session === session && pending.chart === session.chart && count && chart) session.commit(t('quick-curve.commit'), chart);
  }
  cancel() { clearTimeout(this.timer); this.pending = null; this.active = false; this.notes = null; this.chart = null; this.key = null; this.timeline.quickCurveGhost = null; this.timeline.quickCurveLabel = ''; this.invalidate(); }
}
