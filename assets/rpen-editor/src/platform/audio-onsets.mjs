import { checkAgentSignal } from '../core/agent-commands.mjs';
import { t } from '../i18n.mjs';

const yieldFrame = () => new Promise(resolve => setTimeout(resolve, 0));

export async function onsetAudioWindow(buffer, startSeconds, endSeconds, signal, contextSeconds = 1) {
  checkAgentSignal(signal);
  if (!buffer?.getChannelData || startSeconds < 0 || !(endSeconds > startSeconds) || endSeconds > buffer.duration + .001 || endSeconds - startSeconds > 180 || buffer.sampleRate < 8000 || buffer.sampleRate > 192000 || !Number.isInteger(buffer.sampleRate) || buffer.numberOfChannels < 1 || buffer.numberOfChannels > 8) throw Error(t('audio-onsets.s4b2fbb5'));
  if (!Number.isFinite(contextSeconds) || contextSeconds < 0 || contextSeconds > 16) throw Error(t('audio-onsets.s9d048de'));
  const first = Math.floor(Math.max(0, startSeconds - contextSeconds) * 100) / 100, begin = Math.round(first * buffer.sampleRate), last = Math.min(buffer.length, Math.ceil((endSeconds + contextSeconds) * buffer.sampleRate));
  const channels = Array.from({ length: buffer.numberOfChannels }, (unused, index) => buffer.getChannelData(index)), energy = channels.map(() => 0); let mixedEnergy = 0;
  for (let position = begin; position < last; position += 16) {
    let sum = 0; for (let channel = 0; channel < channels.length; channel++) { const value = channels[channel][position]; energy[channel] += value * value; sum += value; } mixedEnergy += (sum / channels.length) ** 2;
    if ((position - begin) % 262144 === 0) { checkAgentSignal(signal); await yieldFrame(); }
  }
  const strongest = energy.indexOf(Math.max(...energy)), preserveChannel = mixedEnergy < energy[strongest] * .1;
  const mono = new Float32Array(last - begin);
  for (let index = 0; index < mono.length; index++) {
    let value = 0; if (preserveChannel) value = channels[strongest][begin + index]; else for (const channel of channels) value += channel[begin + index] / channels.length;
    if (!Number.isFinite(value)) throw Error(t('audio-onsets.sb24e4bd')); mono[index] = value;
    if (index % 262144 === 0) { checkAgentSignal(signal); await yieldFrame(); }
  }
  checkAgentSignal(signal); return { samples: mono, startSeconds: begin / buffer.sampleRate, sampleRate: buffer.sampleRate, channelMode: preserveChannel ? 'strongest-channel' : 'mono-mix' };
}

export class LocalOnsetDetector {
  constructor(bridge = globalThis.rpenAudioOnsets) { this.bridge = bridge?.version === 1 ? bridge : null; }
  async call(action, ...args) {
    if (!this.bridge) throw Error(t('audio-onsets.se564e86'));
    const response = await this.bridge.call(action, ...args); if (!response.ok) throw Object.assign(Error(response.error.message), { name: response.error.name }); return response.result;
  }
  async analyze(buffer, options, signal, progress = () => {}) {
    checkAgentSignal(signal); const id = crypto.randomUUID(); let token;
    const cancel = () => { if (token) this.call('cancel', token).catch(() => {}); }, remove = this.bridge?.onProgress?.(value => { if (value.id === id) progress(value); }); signal?.addEventListener('abort', cancel, { once: true });
    try {
      progress({ stage: 'encoding', message: t('audio-onsets.s0eaf51c') }); const window = await onsetAudioWindow(buffer, options.startSeconds, options.endSeconds, signal); checkAgentSignal(signal);
      token = await this.call('begin', { id, sampleRate: window.sampleRate, frames: window.samples.length, device: options.device ?? 'auto' }); checkAgentSignal(signal);
      const bytes = new Uint8Array(window.samples.buffer);
      for (let offset = 0; offset < bytes.length; offset += 1024 ** 2) { checkAgentSignal(signal); await this.call('write', token, offset, bytes.subarray(offset, offset + 1024 ** 2)); }
      const result = await this.call('execute', token); token = null; checkAgentSignal(signal);
      const encoded = await this.call('read', result.cacheId); checkAgentSignal(signal);
      if (!(encoded instanceof Uint8Array) || encoded.length !== result.frames * 4 || result.frameRate !== 100) throw Error(t('audio-onsets.s907237a'));
      const view = new DataView(encoded.buffer, encoded.byteOffset, encoded.byteLength), salience = Float32Array.from({ length: result.frames }, (unused, index) => view.getFloat32(index * 4, true));
      if (salience.some(value => !Number.isFinite(value) || value < 0 || value > 1)) throw Error(t('audio-onsets.s443d5a1'));
      return { ...result, salience, startSeconds: window.startSeconds, channelMode: window.channelMode };
    } finally { signal?.removeEventListener('abort', cancel); remove?.(); if (token) await this.call('cancel', token).catch(() => {}); }
  }
}
