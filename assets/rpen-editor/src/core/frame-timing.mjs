export function validFpsLimit(value) {
  return value === 0 || Number.isFinite(value) && value > 10;
}

export class FrameLimiter {
  constructor() {
    this.limit = null;
    this.nextFrame = null;
  }

  shouldRender(timestamp, limit = 0) {
    if (!validFpsLimit(limit)) limit = 0;
    if (!limit) { this.limit = 0; this.nextFrame = null; return true; }
    if (this.limit !== limit || this.nextFrame === null) {
      this.limit = limit;
      this.nextFrame = timestamp;
    }
    const interval = 1000 / limit;
    const tolerance = Math.min(0.1, interval / 100);
    if (timestamp + tolerance < this.nextFrame) return false;
    this.nextFrame = timestamp < this.nextFrame ? this.nextFrame + interval : timestamp + interval - ((timestamp - this.nextFrame) % interval);
    return true;
  }
}
