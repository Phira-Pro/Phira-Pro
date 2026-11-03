import { upperBound } from './beat.mjs';

export const HOLD_HIT_INTERVAL = 20 / 120;

export class HitGeometryCache {
  constructor() { this.entries = new Map(); this.frame = 0; }
  begin(scene, positionScale) {
    if (this.chart !== scene.chart || this.tempo !== scene.tempo || this.scaleX !== positionScale.x || this.scaleY !== positionScale.y) {
      this.entries.clear(); this.chart = scene.chart; this.tempo = scene.tempo; this.scaleX = positionScale.x; this.scaleY = positionScale.y;
    }
    this.frame++;
  }
  sample(entry, time, compute) {
    let times = this.entries.get(entry); if (!times) { times = new Map(); this.entries.set(entry, times); }
    let cached = times.get(time); if (!cached) { cached = { value: compute() }; times.set(time, cached); }
    cached.frame = this.frame; return cached.value;
  }
  end() {
    for (const [entry, times] of this.entries) {
      for (const [time, cached] of times) if (cached.frame !== this.frame) times.delete(time);
      if (!times.size) this.entries.delete(entry);
    }
  }
}

export function recentHits(runtime, seconds, since, lifetime = 2 / 3) {
  const first = upperBound(runtime.hitTimes, Math.max(seconds - lifetime, since), entry => entry.start);
  const last = upperBound(runtime.hitTimes, seconds, entry => entry.start);
  const hits = [];
  for (let index = first; index < last; index++) {
    const entry = runtime.hitTimes[index];
    if (entry.note.type !== 2 && !entry.note.isFake) hits.push({ entry, time: entry.start, seed: index });
  }
  for (const { item: entry, index } of runtime.holdIndex.query(seconds - lifetime, seconds)) {
    if (entry.note.isFake) continue;
    const firstPulse = Math.max(0, Math.floor((Math.max(seconds - lifetime, since) - entry.start) / HOLD_HIT_INTERVAL) + 1);
    const lastPulse = Math.min(Math.floor((seconds - entry.start) / HOLD_HIT_INTERVAL), Math.ceil((entry.end - entry.start) / HOLD_HIT_INTERVAL) - 1);
    for (let pulse = firstPulse; pulse <= lastPulse; pulse++) hits.push({ entry, time: entry.start + pulse * HOLD_HIT_INTERVAL, seed: index * 65537 + pulse });
  }
  return hits;
}
