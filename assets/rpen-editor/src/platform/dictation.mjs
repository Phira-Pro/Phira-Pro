export const localDictationAvailable = () => globalThis.rpenDictation?.version === 1;

export async function dictationCall(action, ...args) {
  if (!localDictationAvailable()) throw Error('Local dictation currently requires the desktop app');
  const response = await globalThis.rpenDictation.call(action, ...args);
  if (!response.ok) throw Object.assign(Error(response.error.message), response.error);
  return response.result;
}

export class MicrophoneCapture {
  constructor(onAudio) { this.onAudio = onAudio; this.stopped = false; }
  async start() {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true }, video: false });
    if (this.stopped) { stream.getTracks().forEach(track => track.stop()); return; }
    this.stream = stream;
    try {
      const context = new AudioContext({ sampleRate: 16000 }); this.context = context;
      await context.audioWorklet.addModule(new URL('./dictation-worklet.mjs', import.meta.url));
      if (this.stopped) return;
      this.source = context.createMediaStreamSource(stream);
      this.processor = new AudioWorkletNode(context, 'rpen-dictation-capture');
      this.processor.port.onmessage = event => { if (event.data?.flushed) this.flushed?.(); else if (!this.stopped) this.onAudio(event.data, context.sampleRate); };
      const silent = context.createGain(); silent.gain.value = 0; this.silent = silent;
      this.source.connect(this.processor); this.processor.connect(silent); silent.connect(context.destination);
      await context.resume();
    } catch (error) { await this.stop(); throw error; }
  }
  async stop() {
    if (this.stopping) return this.stopping;
    if (this.stopped) return;
    this.stopping = (async () => {
      if (this.processor && this.context?.state === 'running') await new Promise(resolve => { const timer = setTimeout(resolve, 200); this.flushed = () => { clearTimeout(timer); resolve(); }; this.processor.port.postMessage('flush'); });
      this.stopped = true;
      this.stream?.getTracks().forEach(track => track.stop());
      this.source?.disconnect(); this.processor?.disconnect(); this.silent?.disconnect();
      if (this.processor) this.processor.port.onmessage = null;
      await this.context?.close().catch(() => {});
    })();
    return this.stopping;
  }
}
