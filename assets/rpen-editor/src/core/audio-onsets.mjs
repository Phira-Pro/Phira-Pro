import { t } from '../i18n.mjs';
export const ONSET_DETECTORS = Object.freeze([{ id: 'mapper', name: t('audio-onsets.sb1c5f10') }, { id: 'ddc', name: t('audio-onsets.secb8fe1') }, { id: 'flux', name: t('audio-onsets.sc5fc9c5') }]);

export function selectOnsetPeaks({ salience, frameRate, startSeconds: origin = 0 }, { startSeconds, endSeconds, minConfidence = .15, minGap = .08 } = {}) {
  if (!salience?.length || !Number.isFinite(frameRate) || frameRate <= 0 || !Number.isFinite(origin) || !Number.isFinite(startSeconds) || !(endSeconds > startSeconds) || !Number.isFinite(minConfidence) || minConfidence < 0 || minConfidence > 1 || !Number.isFinite(minGap) || minGap < .02 || minGap > 2) throw Error(t('audio-onsets.s2712f36'));
  const smooth = new Float32Array(salience.length), weights = [.08, .54, 1, .54, .08], candidates = [];
  for (let index = 0; index < salience.length; index++) {
    if (!Number.isFinite(salience[index]) || salience[index] < 0 || salience[index] > 1) throw Error(t('audio-onsets.sadc67cf'));
    for (let offset = -2; offset <= 2; offset++) smooth[index] += (salience[index + offset] ?? 0) * weights[offset + 2] / 2.24;
  }
  for (let index = 0; index < salience.length; index++) {
    if (smooth[index] <= (smooth[index - 1] ?? 0) || smooth[index] < (smooth[index + 1] ?? 0)) continue;
    let peak = index; for (let nearby = Math.max(0, index - 2); nearby <= Math.min(salience.length - 1, index + 2); nearby++) if (salience[nearby] > salience[peak]) peak = nearby;
    const seconds = origin + peak / frameRate, score = salience[peak];
    if (seconds < startSeconds - 1e-7 || seconds >= endSeconds - 1e-7 || score < minConfidence) continue;
    candidates.push({ seconds: Number(seconds.toFixed(6)), confidence: Number(score.toFixed(5)), strength: Number(score.toFixed(5)), band: 'mixed' });
  }
  const selected = [], occupied = new Set(); candidates.sort((left, right) => right.strength - left.strength || left.seconds - right.seconds);
  const gap = minGap * frameRate;
  for (const candidate of candidates) {
    const frame = Math.round((candidate.seconds - origin) * frameRate); if (occupied.has(frame)) continue; selected.push(candidate);
    for (let blocked = Math.floor(frame - gap) + 1; blocked < frame + gap - 1e-7; blocked++) occupied.add(blocked);
  }
  return selected.sort((left, right) => left.seconds - right.seconds);
}

export function estimateOnsetBpms(salience, frameRate) {
  const mean = salience.reduce((sum, value) => sum + value, 0) / salience.length, values = Array.from(salience, value => Math.max(0, value - mean)), scores = [];
  for (let bpm = 60; bpm <= 240; bpm++) {
    const lag = Math.round(60 / bpm * frameRate); let product = 0, first = 0, second = 0;
    for (let index = lag; index < values.length; index++) { product += values[index] * values[index - lag]; first += values[index] ** 2; second += values[index - lag] ** 2; }
    scores.push({ bpm, score: product / Math.max(1e-12, Math.sqrt(first * second)) });
  }
  const selected = []; scores.sort((left, right) => right.score - left.score);
  for (const candidate of scores) { if (candidate.score > .01 && selected.every(entry => Math.abs(entry.bpm - candidate.bpm) > 3)) selected.push(candidate); if (selected.length === 5) break; }
  return selected;
}
