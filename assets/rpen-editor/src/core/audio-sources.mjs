import { t } from '../i18n.mjs';
export const STEM_NAMES = Object.freeze({ original: t('audio-sources.sdd1d7cc'), vocals: t('audio-sources.s84deb81'), drums: t('audio-sources.s11edfbd'), bass: t('audio-sources.s41b2e98'), other: t('audio-sources.s531c78e'), guitar: t('audio-sources.s3ceb469'), piano: t('audio-sources.sc355303') });
export const SEPARATION_MODELS = Object.freeze([
  { id: 'htdemucs', name: t('audio-sources.se977b25'), stems: ['drums', 'bass', 'other', 'vocals'] },
  { id: 'htdemucs_6s', name: t('audio-sources.s5d99ee7'), stems: ['drums', 'bass', 'other', 'vocals', 'guitar', 'piano'] },
]);

export const SOURCE_COLORS = Object.freeze(['#5db9d4', '#e7b55e', '#bb8ce8', '#e78396', '#6fce9b', '#86a8ef', '#db96ce', '#b5c767']);
export const audioSourceColor = index => SOURCE_COLORS[index] ?? `hsl(${Math.round(index * 137.508) % 360}, 64%, 65%)`;

export function sourceAudioRange(source, start, end) {
  const buffer = source.buffer;
  if (!buffer?.getChannelData || !Number.isFinite(start) || !Number.isFinite(end) || end <= start) throw Error(t('audio-sources.s5e77333'));
  if (start < -.001 || end > buffer.duration + .001) throw Error(t('audio-sources.s3c5e513'));
  return { startSeconds: Math.max(0, start), endSeconds: Math.min(buffer.duration, end), offsetSeconds: 0 };
}
