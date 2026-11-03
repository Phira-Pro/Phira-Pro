class DictationCapture extends AudioWorkletProcessor {
  constructor() { super(); this.buffer = new Float32Array(2048); this.offset = 0; this.finished = false; this.port.onmessage = () => { if (this.offset) this.port.postMessage(this.buffer.slice(0, this.offset)); this.offset = 0; this.finished = true; this.port.postMessage({ flushed: true }); }; }
  process(inputs) {
    if (this.finished) return false;
    const channels = inputs[0]; if (!channels?.length) return true;
    const frames = channels[0].length;
    for (let frame = 0; frame < frames; frame++) {
      let sample = 0; for (const channel of channels) sample += channel[frame] ?? 0;
      this.buffer[this.offset++] = Math.max(-1, Math.min(1, sample / channels.length));
      if (this.offset === this.buffer.length) { this.port.postMessage(this.buffer, [this.buffer.buffer]); this.buffer = new Float32Array(2048); this.offset = 0; }
    }
    return true;
  }
}
registerProcessor('rpen-dictation-capture', DictationCapture);
