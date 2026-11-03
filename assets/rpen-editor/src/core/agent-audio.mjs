import { checkAgentSignal } from './agent-commands.mjs';
import { t } from '../i18n.mjs';

const yieldFrame = () => new Promise(resolve => setTimeout(resolve, 0));
const cache = new WeakMap();
const median = values => { const sorted = [...values].sort((left, right) => left - right); return sorted.length ? sorted[Math.floor(sorted.length / 2)] : 0; };
const clamp = (value, low, high) => Math.max(low, Math.min(high, value));

function fftPlan(size) {
  const reverse = new Uint16Array(size), window = new Float64Array(size), cosine = new Float64Array(size / 2), sine = new Float64Array(size / 2);
  for (let index = 0; index < size; index++) {
    let source = index, target = 0; for (let bit = 1; bit < size; bit <<= 1) { target = (target << 1) | (source & 1); source >>= 1; } reverse[index] = target;
    window[index] = .5 - .5 * Math.cos(2 * Math.PI * index / (size - 1));
    if (index < size / 2) { cosine[index] = Math.cos(-2 * Math.PI * index / size); sine[index] = Math.sin(-2 * Math.PI * index / size); }
  }
  return { reverse, window, cosine, sine };
}
function transform(real, imaginary, plan) {
  const size = real.length;
  for (let index = 1; index < size; index++) if (index < plan.reverse[index]) { const other = plan.reverse[index]; [real[index], real[other]] = [real[other], real[index]]; [imaginary[index], imaginary[other]] = [imaginary[other], imaginary[index]]; }
  for (let length = 2; length <= size; length <<= 1) {
    const half = length / 2, stride = size / length;
    for (let start = 0; start < size; start += length) for (let offset = 0; offset < half; offset++) {
      const target = start + offset, paired = target + half, twiddle = offset * stride;
      const horizontal = real[paired] * plan.cosine[twiddle] - imaginary[paired] * plan.sine[twiddle], vertical = real[paired] * plan.sine[twiddle] + imaginary[paired] * plan.cosine[twiddle];
      real[paired] = real[target] - horizontal; imaginary[paired] = imaginary[target] - vertical; real[target] += horizontal; imaginary[target] += vertical;
    }
  }
}

export async function analyzeAgentAudio(buffer, { startSeconds, endSeconds, sensitivity = 1.4, minGap = .08, focus = 'all', minConfidence = .35, signal } = {}) {
  checkAgentSignal(signal);
  if (!buffer?.getChannelData || !(endSeconds > startSeconds) || startSeconds < 0 || endSeconds > buffer.duration + .001 || endSeconds - startSeconds > 180) throw Error(t('agent-audio.s4b2fbb5'));
  if (!['all', 'percussion', 'low', 'melody'].includes(focus)) throw Error(t('agent-audio.sc899767'));
  const key = JSON.stringify([startSeconds, endSeconds, sensitivity, minGap, focus, minConfidence]); let entries = cache.get(buffer);
  if (entries?.has(key)) return structuredClone(entries.get(key));
  const sampleRate = Math.min(22050, buffer.sampleRate), ratio = buffer.sampleRate / sampleRate, size = 1024, hop = Math.max(1, Math.round(sampleRate * .01));
  const start = Math.max(0, Math.floor((startSeconds - .3) * sampleRate)), end = Math.min(Math.ceil(buffer.duration * sampleRate), Math.ceil((endSeconds + .1) * sampleRate));
  const channels = Array.from({ length: Math.min(8, buffer.numberOfChannels) }, (unused, index) => buffer.getChannelData(index)), plan = fftPlan(size);
  const real = new Float64Array(size), imaginary = new Float64Array(size), power = new Float64Array(size / 2), bands = 48, previous = new Float64Array(bands), levels = new Float64Array(bands).fill(.02);
  const ranges = Array.from({ length: bands }, (unused, index) => { const low = 35 * (sampleRate / 2 / 35) ** (index / bands), high = 35 * (sampleRate / 2 / 35) ** ((index + 1) / bands); return { low: Math.max(1, Math.floor(low * size / sampleRate)), high: Math.min(size / 2, Math.max(2, Math.ceil(high * size / sampleRate))), frequency: Math.sqrt(low * high) }; });
  const frames = [], flux = [], energies = [];
  for (let center = start, frame = 0; center < end; center += hop, frame++) {
    power.fill(0); let energy = 0;
    for (const channel of channels) {
      imaginary.fill(0);
      for (let index = 0; index < size; index++) {
        const position = Math.floor((center - size / 2 + index) * ratio); let value = 0, count = 0;
        for (let source = Math.max(0, position); source < Math.min(channel.length, Math.ceil(position + ratio)); source++) { value += channel[source]; count++; }
        value /= count || 1; energy += value * value / size / channels.length; real[index] = value * plan.window[index];
      }
      transform(real, imaginary, plan);
      for (let bin = 1; bin < power.length; bin++) power[bin] += (real[bin] ** 2 + imaginary[bin] ** 2) / channels.length;
    }
    const current = new Float64Array(bands); let novelty = 0, weightTotal = 0, bass = 0, treble = 0;
    for (let band = 0; band < bands; band++) {
      const range = ranges[band]; let value = 0;
      for (let bin = range.low; bin < range.high; bin++) value += Math.log1p(100 * Math.sqrt(power[bin]) / (size * .25));
      value /= Math.max(1, range.high - range.low);
      current[band] = Math.min(4, value / Math.max(.02, levels[band]));
      const baseline = Math.max(previous[band], previous[Math.max(0, band - 1)] * .8, previous[Math.min(bands - 1, band + 1)] * .8);
      const weight = focus === 'low' ? range.frequency < 300 ? 1 : .03 : focus === 'percussion' ? range.frequency < 180 || range.frequency > 2200 ? 1 : .15 : focus === 'melody' ? range.frequency >= 180 && range.frequency <= 3500 ? 1 : .05 : 1;
      const increase = Math.max(0, current[band] - baseline) * weight;
      novelty += increase; weightTotal += weight;
      levels[band] = Math.max(value, levels[band] * .995, .02); if (range.frequency < 300) bass += increase; else if (range.frequency > 1500) treble += increase;
    }
    previous.set(current); flux.push(novelty / weightTotal); energies.push(Math.sqrt(energy)); frames.push({ seconds: center / sampleRate, bass, treble });
    if (frame % 24 === 0) { checkAgentSignal(signal); await yieldFrame(); }
  }
  const smooth = flux.map((value, index) => ((flux[index - 1] ?? value) + 2 * value + (flux[index + 1] ?? value)) / 4);
  const candidates = [], maximumEnergy = energies.reduce((maximum, value) => Math.max(maximum, value), 0);
  const fineHop = Math.max(1, Math.round(buffer.sampleRate * .002)), fineStart = Math.max(0, Math.floor((startSeconds - .1) * buffer.sampleRate)), fine = [];
  for (let source = fineStart, index = 0; source < Math.min(buffer.length, Math.ceil((endSeconds + .08) * buffer.sampleRate)); source += fineHop, index++) {
    let energy = 0; for (const channel of channels) for (let offset = source; offset < Math.min(channel.length, source + fineHop); offset++) energy += channel[offset] ** 2 / fineHop / channels.length;
    fine.push(energy); if (index % 3000 === 0) { checkAgentSignal(signal); await yieldFrame(); }
  }
  const refine = seconds => {
    const center = Math.round((seconds * buffer.sampleRate - fineStart) / fineHop), low = Math.max(1, center - 25), high = Math.min(fine.length - 1, center + 25);
    let attack = center, increase = 0;
    for (let index = low; index <= high; index++) { const difference = fine[index] - fine[index - 1]; if (difference > increase) { increase = difference; attack = index; } }
    const local = fine.slice(low, high + 1).reduce((sum, value) => sum + value, 0) / Math.max(1, high - low + 1);
    if (increase < Math.max(1e-9, local * .25)) return seconds;
    return (fineStart + attack * fineHop) / buffer.sampleRate;
  };
  for (let index = 1; index < smooth.length - 1; index++) {
    if (smooth[index] <= smooth[index - 1] || smooth[index] < smooth[index + 1] || energies[index] < Math.max(1e-7, maximumEnergy * .0003)) continue;
    const neighborhood = smooth.slice(Math.max(0, index - 15), Math.min(smooth.length, index + 16)), center = median(neighborhood), deviation = median(neighborhood.map(value => Math.abs(value - center)));
    const mean = neighborhood.reduce((sum, value) => sum + value, 0) / neighborhood.length, threshold = center + sensitivity * Math.max(deviation * 1.4826, mean * .15, .0005);
    if (smooth[index] <= threshold) continue;
    const confidence = clamp((smooth[index] - threshold) / Math.max(smooth[index], .001), 0, 1), seconds = refine(frames[index].seconds);
    if (confidence < minConfidence || seconds < startSeconds - .00001 || seconds >= endSeconds) continue;
    candidates.push({ seconds, confidence, score: smooth[index] - threshold, band: frames[index].bass > frames[index].treble * 1.5 ? 'low' : frames[index].treble > frames[index].bass * 1.5 ? 'high' : 'mixed' });
  }
  candidates.sort((left, right) => right.score - left.score); const selected = [];
  for (const candidate of candidates) if (selected.every(entry => Math.abs(entry.seconds - candidate.seconds) >= minGap)) selected.push(candidate);
  selected.sort((left, right) => left.seconds - right.seconds); const maximum = selected.reduce((value, peak) => Math.max(value, peak.score), .00001);
  const peaks = selected.map(entry => ({ seconds: Number(entry.seconds.toFixed(5)), strength: Number((entry.score / maximum).toFixed(4)), confidence: Number(entry.confidence.toFixed(4)), band: entry.band }));
  const bpms = [];
  for (let bpm = 60; bpm <= 240; bpm++) {
    const lag = Math.round(60 / bpm * sampleRate / hop); let product = 0, firstEnergy = 0, secondEnergy = 0;
    for (let index = lag; index < smooth.length; index++) { product += smooth[index] * smooth[index - lag]; firstEnergy += smooth[index] ** 2; secondEnergy += smooth[index - lag] ** 2; }
    bpms.push({ bpm, score: product / Math.max(1e-12, Math.sqrt(firstEnergy * secondEnergy)) });
  }
  bpms.sort((left, right) => right.score - left.score); const estimatedBpms = [];
  for (const candidate of bpms) { if (candidate.score > .01 && estimatedBpms.every(entry => Math.abs(entry.bpm - candidate.bpm) > 3)) estimatedBpms.push(candidate); if (estimatedBpms.length === 5) break; }
  checkAgentSignal(signal);
  const result = { peaks, estimatedBpms, sampleRate: buffer.sampleRate, startSeconds, endSeconds, focus, method: 'local-multiband-spectral-flux',
    caution: t('agent-audio.s6c19128'), statistics: { frames: frames.length, candidates: candidates.length, selected: peaks.length, detectorVersion: 2 } };
  if (!entries) { entries = new Map(); cache.set(buffer, entries); } entries.set(key, structuredClone(result)); if (entries.size > 4) entries.delete(entries.keys().next().value);
  return result;
}

export function quantizeAgentPeaks(peaks, { division, start, end, mode = 'adaptive', tolerance = .2 }) {
  const selected = new Map();
  for (const peak of peaks) {
    const nearest = Math.round(peak.beat * division) / division;
    const beat = mode === 'nearest' || mode === 'adaptive' && Math.abs(nearest - peak.beat) * division <= tolerance ? nearest : peak.beat;
    if (beat < start || beat >= end) continue;
    const key = Math.round(beat * 19200), previous = selected.get(key);
    if (!previous || peak.strength > previous.strength) selected.set(key, { ...peak, beat });
  }
  return [...selected.values()].sort((left, right) => left.beat - right.beat);
}
