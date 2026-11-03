class HitSoundImpulses extends AudioWorkletProcessor {
  constructor(options) {
    super(); this.frames = options.processorOptions.frames; this.next = 0; this.carry = 0; this.alive = true; this.shift = 0;
    this.port.onmessage = event => { if (event.data === 'stop') this.alive = false; else this.shift = event.data.shift; };
  }
  process(inputs, outputs) {
    if (!this.alive) return false;
    const output = outputs[0][0]; output[0] += this.carry; this.carry = 0;
    while (this.next < this.frames.length && this.frames[this.next] + this.shift < currentFrame + output.length) {
      const offset = Math.max(0, this.frames[this.next++] + this.shift - currentFrame), first = Math.floor(offset), fraction = offset - first;
      output[first] += 1 - fraction;
      if (first + 1 < output.length) output[first + 1] += fraction; else this.carry += fraction;
    }
    return true;
  }
}

registerProcessor('rpen-hit-impulses', HitSoundImpulses);
