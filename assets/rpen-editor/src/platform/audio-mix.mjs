import { checkAgentSignal } from '../core/agent-commands.mjs';
import { t } from '../i18n.mjs';

const yieldFrame = () => globalThis.scheduler?.yield ? globalThis.scheduler.yield() : new Promise(resolve => setTimeout(resolve, 0));

export async function mixAudioSources(entries, original, context, signal) {
  checkAgentSignal(signal);
  if (!entries.length || !original?.length || !context?.createBuffer) throw Error(t('audio-mix.s10cfdf7'));
  const channels = Math.max(...entries.map(entry => entry.buffer.numberOfChannels)), buffer = context.createBuffer(channels, original.length, original.sampleRate);
  let peak = 1, yieldAt = performance.now() + 6;
  for (let channel = 0; channel < channels; channel++) {
    const output = buffer.getChannelData(channel), inputs = entries.map(entry => ({ data: entry.buffer.getChannelData(Math.min(channel, entry.buffer.numberOfChannels - 1)), ratio: entry.buffer.sampleRate / original.sampleRate }));
    for (let start = 0; start < output.length; start += 32768) {
      checkAgentSignal(signal); const end = Math.min(output.length, start + 32768);
      for (const { data, ratio } of inputs) {
        if (ratio === 1) { for (let index = start; index < Math.min(end, data.length); index++) output[index] += data[index]; }
        else for (let index = start; index < end; index++) { const position = index * ratio, first = Math.floor(position); if (first >= data.length) break; const fraction = position - first; output[index] += data[first] * (1 - fraction) + (data[first + 1] ?? data[first]) * fraction; }
      }
      for (let index = start; index < end; index++) { if (!Number.isFinite(output[index])) throw Error(t('audio-mix.s8680ba7')); peak = Math.max(peak, Math.abs(output[index])); }
      if (performance.now() >= yieldAt) { await yieldFrame(); yieldAt = performance.now() + 6; }
    }
  }
  if (peak > 1) for (let channel = 0; channel < channels; channel++) {
    const output = buffer.getChannelData(channel);
    for (let start = 0; start < output.length; start += 65536) { checkAgentSignal(signal); for (let index = start; index < Math.min(output.length, start + 65536); index++) output[index] /= peak; if (performance.now() >= yieldAt) { await yieldFrame(); yieldAt = performance.now() + 6; } }
  }
  checkAgentSignal(signal); return { buffer, gain: 1 / peak };
}
