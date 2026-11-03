import { beatValue } from './beat.mjs';
import { TempoMap } from './tempo.mjs';
import { t } from '../i18n.mjs';

export const MAPPER_SOURCE_REVISION = '0e2b0e387aab4b35c64b0b11b12d47578dea7587';
export const MAPPER_MODELS = Object.freeze([
  { id: 'v32', name: 'V32', repo: 'OliBomby/Mapperatorinator-v32', revision: '74f22583400d259bf424819e11027c17933efe54' },
  { id: 'v32-mini', name: 'V32 Mini', repo: 'OliBomby/Mapperatorinator-v32-mini', revision: '7807f0dc70cab671be012e1f5ddf945b0b8b7278' },
]);
export const MAPPER_DEFAULTS = Object.freeze({ mapperModel: 'v32', mapperMode: 3, difficulty: 5, seed: 0, useChartTiming: true });
const headTypes = new Set(['circle', 'slider_head', 'hold_note', 'spinner', 'drumroll', 'denden']);

export function validateMapperSettings(settings) {
  if (!MAPPER_MODELS.some(model => model.id === settings.mapperModel) || ![0, 3].includes(settings.mapperMode) || !Number.isFinite(settings.difficulty) || settings.difficulty < .1 || settings.difficulty > 15 || !Number.isSafeInteger(settings.seed) || settings.seed < 0 || settings.seed > 0xffffffff || typeof settings.useChartTiming !== 'boolean') throw Error(t('audio-mapper.s5579aef'));
  return settings;
}

export function mapperTiming(chart, sourceOffset = 0) {
  const tempo = new TempoMap(chart.BPMList), offset = Number(chart.META?.offset ?? 0) / 1000 - sourceOffset;
  return chart.BPMList.map(entry => ({ seconds: tempo.seconds(beatValue(entry.startTime)) + offset, bpm: Number(entry.bpm) })).sort((left, right) => left.seconds - right.seconds);
}

export function selectMapperPeaks(objects, { startSeconds, endSeconds, minGap = .02 }) {
  if (!Array.isArray(objects) || objects.length > 60000 || !Number.isFinite(startSeconds) || !(endSeconds > startSeconds) || !Number.isFinite(minGap) || minGap < .02 || minGap > 2) throw Error(t('audio-mapper.s25ce911'));
  const grouped = new Map();
  for (const object of objects) {
    if (!Number.isFinite(object.seconds) || !headTypes.has(object.type)) throw Error(t('audio-mapper.s56b6baf'));
    if (object.seconds < startSeconds - 1e-7 || object.seconds >= endSeconds - 1e-7) continue;
    const key = Math.round(object.seconds * 1000000), previous = grouped.get(key);
    if (previous) previous.multiplicity++;
    else grouped.set(key, { seconds: object.seconds, strength: 1, sourceType: object.type, multiplicity: 1 });
  }
  const selected = [];
  for (const peak of [...grouped.values()].sort((left, right) => left.seconds - right.seconds)) if (!selected.length || peak.seconds - selected.at(-1).seconds >= minGap - 1e-7) selected.push(peak);
  return selected;
}
