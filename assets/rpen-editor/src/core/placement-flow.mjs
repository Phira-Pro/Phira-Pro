import { upperBound } from './beat.mjs';
import { noteIsAbove } from './chart.mjs';

// An editable, monotone distance axis. Smooth positive sections use the game's
// integral; reverse sections stay on their own side of the judgement
// line. A stopped section retains a small time axis so it can still be edited.
export class PlacementFlow {
  constructor(runtime, tempo) {
    this.runtime = runtime; this.tempo = tempo; this.factor = runtime.line.bpmfactor ?? 1;
    this.notes = new WeakMap(); this.beats = new Map(); this.segments = [];
    const speed = time => runtime.speeds.reduce((sum, track) => sum + track.value(time), 0) * 120;
    const constant = (start, end) => runtime.speeds.every(track => {
      const entry = track.events[Math.max(0, upperBound(track.events, (start + end) / 2, event => event.start) - 1)];
      return !entry || entry.event.start === entry.event.end || end <= entry.start || start >= entry.end;
    });
    const source = [...new Set([0, ...runtime.speeds.flatMap(track => track.events.flatMap(entry => [entry.start, entry.end]))])].sort((a, b) => a - b);
    const cuts = new Set(source);
    for (let index = 1; index < source.length; index++) {
      const start = source[index - 1], end = source[index]; if (constant(start, end)) continue;
      const inset = (end - start) * 1e-9;
      let previous = start + inset, value = speed(previous);
      for (let sample = 1; sample <= 24; sample++) {
        const current = sample === 24 ? end - inset : start + (end - start) * sample / 24, next = speed(current);
        if (value * next < 0) {
          let left = previous, right = current;
          for (let step = 0; step < 36; step++) { const middle = (left + right) / 2; if (speed(middle) * value > 0) left = middle; else right = middle; }
          cuts.add((left + right) / 2);
        } else if (Math.abs(next) < 1e-9 && sample !== 24) cuts.add(current);
        previous = current; value = next;
      }
    }
    this.cuts = [...cuts].sort((a, b) => a - b); this.first = this.cuts[0]; this.last = this.cuts.at(-1);
    this.firstSlope = Math.abs(speed(this.first - 1)) || 120; this.lastSlope = Math.abs(speed(this.last + 1)) || 120;
    let distance = 0;
    for (let index = 1; index < this.cuts.length; index++) {
      const start = this.cuts[index - 1], end = this.cuts[index], floor = runtime.floor(start), change = runtime.floor(end) - floor;
      const stopped = Math.abs(change) < 1e-9 && Math.abs(speed((start + end) / 2)) < 1e-9;
      const length = stopped ? (end - start) * 120 : Math.abs(change);
      const segment = { start, end, floor, distance, limit: distance + length, sign: change < 0 ? -1 : 1,
        linear: stopped || constant(start, end), slope: length / (end - start) };
      if (!segment.linear) {
        const sensitive = runtime.speeds.some(track => track.events.some(entry => entry.start < end && entry.end > start && entry.event.start !== entry.event.end &&
          (entry.event.bezier || [16, 17, 18, 19, 22, 24, 25, 26, 27, 28, 29].includes(entry.event.easingType))));
        // The renderer's fixed-step quadrature can fold back near bounce or
        // elastic boundaries. Keep those exceptional spans editable and cache
        // the inverse once, rather than repeatedly searching an ambiguous root.
        if (sensitive) {
          const count = 256, samples = [0];
          const minimum = (end - start) / count * .0001;
          for (let sample = 1; sample <= count; sample++) samples.push(Math.max(samples[sample - 1] + minimum, segment.sign * (runtime.floor(start + (end - start) * sample / count) - floor)));
          segment.samples = samples; segment.limit = distance + samples.at(-1);
        }
      }
      this.segments.push(segment); distance = segment.limit;
    }
    this.lastDistance = distance; this.zero = this.raw(0);
  }
  raw(seconds) {
    if (seconds <= this.first) return (seconds - this.first) * this.firstSlope;
    if (seconds >= this.last) return this.lastDistance + (seconds - this.last) * this.lastSlope;
    const segment = this.segments[Math.max(0, upperBound(this.segments, seconds, item => item.start) - 1)];
    if (segment.samples) {
      const sample = (seconds - segment.start) / (segment.end - segment.start) * (segment.samples.length - 1), index = Math.min(segment.samples.length - 2, Math.floor(sample));
      return segment.distance + segment.samples[index] + (segment.samples[index + 1] - segment.samples[index]) * (sample - index);
    }
    return segment.distance + (segment.linear ? (seconds - segment.start) * segment.slope : segment.sign * (this.runtime.floor(seconds) - segment.floor));
  }
  distance(seconds) { return this.raw(seconds) - this.zero; }
  seconds(distance) {
    const value = distance + this.zero;
    if (value <= 0) return this.first + value / this.firstSlope;
    if (value >= this.lastDistance) return this.last + (value - this.lastDistance) / this.lastSlope;
    const segment = this.segments[Math.max(0, upperBound(this.segments, value, item => item.distance) - 1)];
    if (segment.samples) {
      const target = value - segment.distance, index = Math.max(0, Math.min(segment.samples.length - 2, upperBound(segment.samples, target, item => item) - 1));
      const fraction = (target - segment.samples[index]) / (segment.samples[index + 1] - segment.samples[index]);
      return segment.start + (segment.end - segment.start) * (index + fraction) / (segment.samples.length - 1);
    }
    if (segment.linear) return segment.start + (value - segment.distance) / segment.slope;
    let left = segment.start, right = segment.end;
    for (let step = 0; step < 42; step++) { const middle = (left + right) / 2; if (this.raw(middle) < value) left = middle; else right = middle; }
    return (left + right) / 2;
  }
  atBeat(beat) {
    if (!this.beats.has(beat)) { if (this.beats.size > 4096) this.beats.clear(); this.beats.set(beat, this.distance(this.tempo.seconds(beat, this.factor))); }
    return this.beats.get(beat);
  }
  note(note) {
    let result = this.notes.get(note);
    if (!result) { result = { startTime: this.atBeatValue(note.startTime), endTime: this.atBeatValue(note.endTime) }; this.notes.set(note, result); }
    return result;
  }
  atBeatValue(beat) { return this.distance(this.tempo.seconds(beat, this.factor)); }
}

export function placementFlowView(flow, seconds, plane, pixelsPerDistance) {
  const currentDistance = flow.distance(seconds), sideAt = y => y <= plane.baseline ? 1 : -1;
  const pointerDistance = (y, side = sideAt(y)) => currentDistance + Math.max(0, (plane.baseline - y) * side) / pixelsPerDistance;
  const vertical = (distance, side) => plane.baseline - side * Math.max(0, distance - currentDistance) * pixelsPerDistance;
  const minimum = plane.baseline < 0 ? -plane.baseline : plane.baseline > plane.height ? plane.baseline - plane.height : 0;
  const maximum = Math.max(Math.abs(plane.baseline), Math.abs(plane.height - plane.baseline));
  const beatRange = [minimum, maximum].map(distance => flow.tempo.beat(flow.seconds(currentDistance + distance / pixelsPerDistance), flow.factor));
  return { flow, currentDistance, pixelsPerDistance, sideAt, pointerDistance,
    vertical: (beat, side = 1) => vertical(flow.atBeat(beat), side),
    noteVertical: (note, endpoint = 'startTime') => vertical(flow.note(note)[endpoint], noteIsAbove(note) ? 1 : -1),
    timeAt: (y, side) => flow.seconds(pointerDistance(y, side)),
    distanceAt: beat => flow.atBeatValue(beat), beatAtDistance: distance => flow.tempo.beat(flow.seconds(distance), flow.factor),
    beatRange: () => [beatRange[0] - 1e-8, beatRange[1] + 1e-8] };
}

export function clipPlacementConnection(start, end, viewport) {
  const dx = end.x - start.x, dy = end.y - start.y;
  let first = 0, last = 1;
  for (const [p, q] of [[-dx, start.x - viewport.left], [dx, viewport.left + viewport.width - start.x], [-dy, start.y - viewport.top], [dy, viewport.top + viewport.height - start.y]]) {
    if (!p) { if (q < 0) return null; continue; }
    const value = q / p; if (p < 0) first = Math.max(first, value); else last = Math.min(last, value);
    if (first > last) return null;
  }
  return [{ x: start.x + dx * first, y: start.y + dy * first }, { x: start.x + dx * last, y: start.y + dy * last }];
}
