export class WaveformEnvelope {
  constructor(samples) {
    this.length = samples.length;
    this.levels = [samples];
    while (samples.length > 1) {
      const next = new Float32Array(Math.ceil(samples.length / 2));
      for (let index = 0; index < next.length; index++) next[index] = Math.max(samples[index * 2], samples[index * 2 + 1] ?? 0);
      this.levels.push(next); samples = next;
    }
  }

  peak(start, end) {
    if (!(end > start) || end <= 0 || start >= this.length) return 0;
    let first = Math.max(0, Math.floor(start)), last = Math.min(this.length, Math.ceil(end));
    let maximum = 0, level = 0;
    while (first < last) {
      const samples = this.levels[level++];
      if (first & 1) maximum = Math.max(maximum, samples[first++]);
      if (last & 1) maximum = Math.max(maximum, samples[--last]);
      first >>>= 1; last >>>= 1;
    }
    return maximum;
  }
}
