import { LocalOnsetDetector, onsetAudioWindow } from './audio-onsets.mjs';
import { checkAgentSignal } from '../core/agent-commands.mjs';
import { t } from '../i18n.mjs';

export class LocalAudioMapper extends LocalOnsetDetector {
  constructor(bridge = globalThis.rpenAudioMapper) { super(bridge); }
  async call(action, ...args) { if (!this.bridge) throw Error(t('audio-mapper.s13e57c1')); return super.call(action, ...args); }
  async analyze(buffer, options, signal, progress = () => {}) {
    checkAgentSignal(signal); if (!this.bridge) throw Error(t('audio-mapper.s13e57c1')); const id = crypto.randomUUID(); let token;
    const cancel = () => { if (token) this.call('cancel', token).catch(() => {}); }, remove = this.bridge?.onProgress?.(value => { if (value.id === id) progress(value); }); signal?.addEventListener('abort', cancel, { once: true });
    try {
      progress({ stage: 'encoding', message: t('audio-mapper.s6a87854') }); const window = await onsetAudioWindow(buffer, options.startSeconds, options.endSeconds, signal, 16); checkAgentSignal(signal);
      token = await this.call('begin', { id, sampleRate: window.sampleRate, frames: window.samples.length, device: options.device ?? 'auto', mapperModel: options.mapperModel, mapperMode: options.mapperMode, difficulty: options.difficulty, seed: options.seed, useChartTiming: options.useChartTiming, timing: options.timing ?? [], startSeconds: options.startSeconds, endSeconds: options.endSeconds, originSeconds: window.startSeconds, sourceDuration: buffer.duration }); checkAgentSignal(signal);
      const bytes = new Uint8Array(window.samples.buffer);
      for (let offset = 0; offset < bytes.length; offset += 1024 ** 2) { checkAgentSignal(signal); await this.call('write', token, offset, bytes.subarray(offset, offset + 1024 ** 2)); }
      const metadata = await this.call('execute', token); token = null; checkAgentSignal(signal);
      const encoded = await this.call('read', metadata.cacheId); checkAgentSignal(signal); if (!(encoded instanceof Uint8Array) || encoded.length > 8 * 1024 ** 2) throw Error(t('audio-mapper.sbd70a76'));
      const result = JSON.parse(new TextDecoder().decode(encoded)); if (result.engine !== 'mapper' || !Array.isArray(result.objects)) throw Error(t('audio-mapper.s3f99fdb')); return { ...result, cacheHit: metadata.cacheHit, channelMode: window.channelMode };
    } finally { signal?.removeEventListener('abort', cancel); remove?.(); if (token) await this.call('cancel', token).catch(() => {}); }
  }
}
