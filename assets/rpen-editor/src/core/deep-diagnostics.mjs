import { TempoMap } from './tempo.mjs';
import { SceneRuntime } from './scene.mjs';
import { EventTrack } from './events.mjs';
import { previewPositionScale, previewNotePosition } from './editor-display.mjs';
import { diagnostic, diagnosticKey } from './chart-diagnostics.mjs';
import { resourceDiagnostics, compatibilityDiagnostics } from './diagnostic-resources.mjs';
import { AXES, PlaneCamera } from '../../assets/mods/spatial-camera/camera.mjs';

const pause = () => new Promise(resolve => setTimeout(resolve, 0));
const fake = note => note.isFake === true || Number(note.isFake) === 1;
function lower(items, seconds) { let a = 0, b = items.length; while (a < b) { const m = (a + b) >>> 1; if (items[m].start < seconds) a = m + 1; else b = m; } return a; }
// Time + X range queries, including an optional minimum Hold end. A stacked
// chart never generates an all-pairs candidate list.
class Nearby {
  constructor(entries) {
    this.groups = new Map();
    for (const entry of entries) { const bucket = Math.floor(entry.note.positionX / 200); if (!this.groups.has(bucket)) this.groups.set(bucket, []); this.groups.get(bucket).push(entry); }
    for (const [key, values] of this.groups) {
      values.sort((a, b) => a.start - b.start); let size = 1; while (size < values.length) size *= 2;
      const min = new Float64Array(size * 2).fill(Infinity), max = new Float64Array(size * 2).fill(-Infinity), end = new Float64Array(size * 2).fill(-Infinity);
      values.forEach((entry, i) => { min[size + i] = max[size + i] = entry.note.positionX; end[size + i] = entry.end; });
      for (let i = size - 1; i; i--) { min[i] = Math.min(min[i * 2], min[i * 2 + 1]); max[i] = Math.max(max[i * 2], max[i * 2 + 1]); end[i] = Math.max(end[i * 2], end[i * 2 + 1]); }
      this.groups.set(key, { values, size, min, max, end });
    }
  }
  find(start, finish, x, minimumEnd = -Infinity, excludeIndex = -1) {
    const bucket = Math.floor(x / 200);
    for (let k = bucket - 1; k <= bucket + 1; k++) {
      const group = this.groups.get(k); if (!group) continue;
      const { values, size, min, max, end } = group, first = lower(values, start), last = lower(values, finish);
      const visit = (node, a, b) => {
        if (b <= first || a >= last || min[node] >= x + 200 || max[node] <= x - 200 || end[node] < minimumEnd) return null;
        if (b - a === 1) return values[a]?.index === excludeIndex ? null : values[a] ?? null;
        const middle = (a + b) >>> 1; return visit(node * 2, a, middle) ?? visit(node * 2 + 1, middle, b);
      };
      const found = visit(1, 0, size); if (found) return found;
    }
    return null;
  }
}
export async function deepDiagnostics(chart, options = {}, progress = () => {}) {
  const issues = [], tempo = new TempoMap(chart.BPMList), scene = new SceneRuntime(); scene.compile(chart, tempo);
  const ratio = options.ratio ?? 1.5, scale = previewPositionScale(ratio), width = Math.min(1350, 900 * ratio), height = Math.min(900, 1350 / ratio);
  const cameraTracks = options.activeMods?.includes('spatial-camera') ? AXES.map(axis => ({ axis, track: new EventTrack(chart.globalEvents?.types?.[`spatial-camera/${axis.type}`]?.columns?.[axis.id]?.events, tempo, 1, axis.fallback) })) : [];
  const camera = seconds => cameraTracks.length ? new PlaneCamera(Object.fromEntries(cameraTracks.map(({ axis, track }) => [axis.id, !track.events.length || seconds < track.events[0].start ? axis.fallback : track.value(seconds)])), { width, height, scale: 1 }) : null;
  const total = scene.lines.reduce((sum, line) => sum + line.notes.length, 0); let completed = 0;
  for (const [lineIndex, runtime] of scene.lines.entries()) {
    const valid = runtime.hitTimes.filter(entry => !fake(entry.note) && Number.isFinite(entry.start) && Number.isFinite(entry.end));
    const indexes = new Map();
    for (const above of [true, false]) {
      const entries = valid.filter(entry => (Number(entry.note.above ?? 1) === 1) === above);
      indexes.set(above, { taps: new Nearby(entries.filter(entry => entry.note.type === 1)), previous: new Nearby(entries.filter(entry => entry.note.type === 3 || entry.note.type === 4)), holds: new Nearby(entries.filter(entry => entry.note.type === 2)), tails: new Nearby(entries.filter(entry => entry.note.type === 2).map(entry => ({ ...entry, start: entry.end }))) });
    }
    const cuts = [], ancestors = new Set(); let ancestor = lineIndex;
    while (scene.lines[ancestor] && !ancestors.has(ancestor)) {
      ancestors.add(ancestor); const line = scene.lines[ancestor].line, factor = line.bpmfactor ?? 1;
      for (const layer of [...(line.eventLayers ?? []), line.extended ?? {}]) for (const events of Object.values(layer ?? {})) if (Array.isArray(events)) for (const event of events) {
        try { cuts.push({ start: tempo.seconds(event.startTime, factor) }, { start: tempo.seconds(event.endTime, factor) }); } catch {}
      }
      ancestor = Number(line.father ?? -1);
    }
    cuts.sort((a, b) => a.start - b.start);
    const stateCache = new Map();
    const sample = (entry, seconds) => {
      if (!stateCache.has(seconds)) { if (stateCache.size >= 2048) stateCache.clear(); stateCache.set(seconds, { state: scene.sampler(seconds, scale)(lineIndex), projection: camera(seconds) }); }
      const { state, projection } = stateCache.get(seconds), local = previewNotePosition(runtime.noteState(entry, state, seconds), runtime.line, scale), angle = state.rotation * Math.PI / 180;
      let point = { x: state.x + local.x * Math.cos(angle) + local.y * Math.sin(angle), y: state.y - local.x * Math.sin(angle) + local.y * Math.cos(angle) };
      if (projection) { const screen = projection.project(point); point = screen ? { x: screen.x - width / 2, y: height / 2 - screen.y } : { x: Infinity, y: Infinity }; }
      const inside = Math.abs(point.x) <= width / 2 + 1e-6 && Math.abs(point.y) <= height / 2 + 1e-6;
      return { inside, width: local.size, visible: inside && state.alpha >= 0 && local.alpha > 0 && local.size !== 0 && entry.start - seconds <= (entry.note.visibleTime ?? 999999) + 1e-8 && !(runtime.line.isCover === 1 && (entry.note.type === 2 ? entry.tail : entry.floor) < state.floor) };
    };
    for (const entry of valid) {
      const where = { line: lineIndex, index: entry.index, beat: tempo.beat(entry.start, runtime.line.bpmfactor ?? 1), path: `notes[${entry.index}]`, deep: true, removable: false };
      const judged = sample(entry, entry.start);
      if (!judged.inside) issues.push(diagnostic('outScreen', 'warning', where));
      if (judged.width === 0 && entry.note.size !== 0) issues.push(diagnostic('zeroWidth', 'warning', where));
      if (judged.inside) {
        // Walk backwards across both uniform probes and event boundaries; unlike
        // one global binary search this also detects non-monotonic visibility.
        const probes = Array.from({ length: 61 }, (_, i) => entry.start - i * .005);
        for (let i = lower(cuts, entry.start - .3); i < cuts.length && cuts[i].start <= entry.start; i++) probes.push(cuts[i].start, cuts[i].start - 1e-7);
        probes.sort((a, b) => b - a); let duration = judged.visible ? .3 : 0, previous = entry.start;
        if (judged.visible) for (const seconds of probes) {
          if (seconds < entry.start - .3 - 1e-8) continue;
          if (!sample(entry, seconds).visible) {
            let a = seconds, b = previous; while (b - a > .001) { const middle = (a + b) / 2; if (sample(entry, middle).visible) b = middle; else a = middle; }
            duration = entry.start - b; break;
          }
          previous = seconds;
        }
        if (duration < .299) issues.push(diagnostic('shortRead', 'info', where, { ms: Math.round(duration * 1000) }));
      }
      if (entry.note.type === 3 || entry.note.type === 4) {
        const group = indexes.get(Number(entry.note.above ?? 1) === 1), x = entry.note.positionX;
        const target = group.taps.find(entry.start + .04 - 1e-8, entry.start + .16 + 1e-8, x);
        if (target) {
          const context = group.previous.find(entry.start - .16, entry.start + 1e-8, x, -Infinity, entry.index) || (entry.note.type === 3 ? group.tails.find(entry.start - 1e-8, entry.start + 1e-8, x) : group.holds.find(-Infinity, entry.start + 1e-8, x, entry.start));
          const elapsed = target.start - entry.start;
          if (!context || entry.note.type === 3 || elapsed <= .04 + 1e-8) issues.push(diagnostic(entry.note.type === 3 ? 'tapAfterFlick' : 'tapAfterDrag', context ? 'info' : entry.note.type === 3 ? 'error' : 'warning', where, { ms: Math.round(elapsed * 1000) }));
        }
      }
      if (++completed % 100 === 0) { progress({ completed, total }); await pause(); }
    }
  }
  for (const check of options.customGlobalChecks ?? []) {
    let url;
    try {
      let source = check.source; if (/^(?:async\s+)?[a-zA-Z_$][\w$]*\s*\(/.test(source) && !/^(?:async\s+)?function\b/.test(source)) source = source.replace(/^(async\s+)?/, '$1function ');
      url = URL.createObjectURL(new Blob(['export default (' + source + ');'], { type: 'text/javascript' })); const validate = (await import(url)).default;
      for (const [index, event] of (chart.globalEvents?.types?.[check.type]?.columns?.[check.column]?.events ?? []).entries()) {
        const where = { globalType: check.type, globalColumn: check.column, index, path: `globalEvents.${check.type}.${check.column}[${index}]`, deep: true };
        try { const result = await validate(structuredClone(event)); if (result === false || typeof result === 'string') issues.push(diagnostic('globalRule', 'error', where, { reason: typeof result === 'string' ? result : check.column })); }
        catch (error) { if (error instanceof ReferenceError) throw error; issues.push(diagnostic('globalRule', 'error', where, { reason: error.message })); }
        if (index % 100 === 0) await pause();
      }
    } catch (error) { issues.push(diagnostic('globalCheckFailure', 'warning', { globalType: check.type, globalColumn: check.column, path: 'globalEvents', deep: true }, { reason: error.message })); }
    finally { if (url) URL.revokeObjectURL(url); }
  }
  issues.push(...resourceDiagnostics(chart, options, true), ...compatibilityDiagnostics(chart, options.target));
  progress({ completed: total, total }); return [...new Map(issues.map(issue => [diagnosticKey(issue), issue])).values()];
}
