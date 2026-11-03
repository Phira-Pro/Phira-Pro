import { pcmWave } from './pcm-wave.mjs';
import { checkAgentSignal, agentAbort } from '../core/agent-commands.mjs';
import { t } from '../i18n.mjs';

export class LocalAudioSeparator {
  constructor(bridge = globalThis.rpenAudioSeparation) { this.bridge = bridge?.version === 1 ? bridge : null; }
  async call(action, ...args) {
    if (!this.bridge) throw Error(t('audio-separation.s54a823b'));
    const response = await this.bridge.call(action, ...args); if (!response.ok) throw Object.assign(Error(response.error.message), { name: response.error.name }); return response.result;
  }
  async separate(buffer, options = {}, signal, progress = () => {}) {
    checkAgentSignal(signal); const id = crypto.randomUUID(); let token;
    const cancel = () => { if (token) this.call('cancel', token).catch(() => {}); }, remove = this.bridge?.onProgress?.(value => { if (value.id === id) progress(value); });
    signal?.addEventListener('abort', cancel, { once: true });
    try {
      progress({ stage: 'encoding', message: t('audio-separation.s9e86d11') });
      if (buffer.numberOfChannels > 2 || buffer.duration > 1800) throw Error(t('audio-separation.s4e51962'));
      const wave = await pcmWave(buffer, () => signal?.aborted); checkAgentSignal(signal); if (!wave) throw agentAbort();
      token = await this.call('begin', { id, model: options.model ?? 'htdemucs', device: options.device ?? 'auto', shifts: options.shifts ?? 1, bytes: wave.size }); checkAgentSignal(signal);
      for (let offset = 0; offset < wave.size; offset += 1024 ** 2) { checkAgentSignal(signal); await this.call('write', token, offset, new Uint8Array(await wave.slice(offset, offset + 1024 ** 2).arrayBuffer())); }
      checkAgentSignal(signal); const result = await this.call('execute', token); token = null; checkAgentSignal(signal); return result;
    } finally { signal?.removeEventListener('abort', cancel); remove?.(); if (token) await this.call('cancel', token).catch(() => {}); }
  }
  async bytes(result, stem, signal) {
    const source = result.sources.find(entry => entry.id === stem); if (!source) throw Error(t('audio-separation.sd0cf85a')); const bytes = new Uint8Array(source.bytes);
    for (let offset = 0; offset < bytes.length;) { checkAgentSignal(signal); const chunk = await this.call('read', result.cacheId, stem, offset, Math.min(1024 ** 2, bytes.length - offset)); if (!chunk?.length) throw Error(t('audio-separation.s6492855')); bytes.set(chunk, offset); offset += chunk.length; }
    checkAgentSignal(signal); return bytes;
  }
}
