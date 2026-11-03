import { analyzeAgentAudio } from '../core/agent-audio.mjs';
import { ONSET_DETECTORS, selectOnsetPeaks, estimateOnsetBpms } from '../core/audio-onsets.mjs';
import { checkAgentSignal } from '../core/agent-commands.mjs';
import { LocalOnsetDetector } from '../platform/audio-onsets.mjs';
import { LocalAudioMapper } from '../platform/audio-mapper.mjs';
import { MAPPER_DEFAULTS, selectMapperPeaks, validateMapperSettings } from '../core/audio-mapper.mjs';
import { t } from '../i18n.mjs';

export class AudioOnsets extends EventTarget {
  constructor({ detector = new LocalOnsetDetector(), mapper = new LocalAudioMapper(), getTiming } = {}) { super(); this.detector = detector; this.mapper = mapper; this.getTiming = getTiming; this.options = { ...MAPPER_DEFAULTS, detector: mapper.bridge ? 'mapper' : detector.bridge ? 'ddc' : 'flux', minConfidence: detector.bridge ? .15 : .35, minGap: mapper.bridge ? .02 : .08, device: 'auto' }; this.cache = new WeakMap(); }
  changed() { this.dispatchEvent(new Event('change')); }
  configure(options) {
    const next = { ...this.options, ...options }; if (options.detector && options.detector !== this.options.detector && options.minConfidence === undefined) next.minConfidence = options.detector === 'ddc' ? .15 : .35;
    if (options.detector && options.detector !== this.options.detector && options.minGap === undefined) next.minGap = options.detector === 'mapper' ? .02 : .08;
    validateMapperSettings(next);
    if (!ONSET_DETECTORS.some(entry => entry.id === next.detector) || !Number.isFinite(next.minConfidence) || next.minConfidence < 0 || next.minConfidence > 1 || !Number.isFinite(next.minGap) || next.minGap < .02 || next.minGap > 2 || !['auto', 'cpu', 'cuda'].includes(next.device)) throw Error(t('audio-onsets.sc5e5034'));
    this.options = next; this.changed();
  }
  clearPreview() { this.preview = null; this.changed(); }
  cancel() { this.controller?.abort(); }
  async analyze(buffer, options, signal) {
    checkAgentSignal(signal); if (this.busy) throw Error(t('audio-onsets.s8d704ac'));
    const settings = { ...this.options, ...Object.fromEntries(Object.entries(options).filter(([, value]) => value !== undefined)) };
    if (settings.detector !== this.options.detector && options.minConfidence === undefined) settings.minConfidence = settings.detector === 'ddc' ? .15 : .35;
    if (settings.detector !== this.options.detector && options.minGap === undefined) settings.minGap = settings.detector === 'mapper' ? .02 : .08;
    if (!ONSET_DETECTORS.some(entry => entry.id === settings.detector)) throw Error(t('audio-onsets.s68bb9c8'));
    if (settings.detector !== 'flux' && settings.focus && settings.focus !== 'all') throw Error(t('audio-onsets.s3e16511'));
    if (settings.detector === 'mapper') { validateMapperSettings(settings); settings.timing = settings.useChartTiming ? options.timing ?? this.getTiming?.(settings.sourceOffset ?? 0) ?? [] : []; }
    this.controller = new AbortController(); const controller = this.controller, cancel = () => controller.abort(); signal?.addEventListener('abort', cancel, { once: true }); if (signal?.aborted) cancel();
    this.busy = true; this.progress = { message: t('audio-onsets.s8180288') }; this.changed();
    try {
      if (settings.detector === 'flux') return { ...await analyzeAgentAudio(buffer, { ...settings, signal: controller.signal }), detector: 'flux', threshold: settings.minConfidence, minGap: settings.minGap };
      let entries = this.cache.get(buffer); if (!entries) { entries = new Map(); this.cache.set(buffer, entries); }
      const key = JSON.stringify([settings.detector, settings.startSeconds, settings.endSeconds, ...(settings.detector === 'mapper' ? [settings.mapperModel, settings.mapperMode, settings.difficulty, settings.seed, settings.timing] : [])]); let raw = entries.get(key), cacheHit = Boolean(raw);
      if (!raw) { raw = await (settings.detector === 'mapper' ? this.mapper : this.detector).analyze(buffer, settings, controller.signal, value => { this.progress = value; this.dispatchEvent(new Event('progress')); }); checkAgentSignal(controller.signal); entries.set(key, raw); if (entries.size > 6) entries.delete(entries.keys().next().value); cacheHit = raw.cacheHit === true; }
      checkAgentSignal(controller.signal);
      if (settings.detector === 'mapper') {
        const peaks = selectMapperPeaks(raw.objects, settings);
        return { peaks, estimatedBpms: [], sampleRate: buffer.sampleRate, startSeconds: settings.startSeconds, endSeconds: settings.endSeconds, detector: 'mapper', method: 'mapperatorinator-rhythm-draft', cacheHit, model: { name: raw.model, revision: raw.modelRevision, sourceRevision: raw.sourceRevision, device: raw.device, gamemode: raw.gamemode, difficulty: raw.difficulty, seed: raw.seed, usedChartTiming: raw.usedChartTiming }, minGap: settings.minGap, statistics: { generatedObjects: raw.objects.length, selected: peaks.length, windows: raw.windows, channelMode: raw.channelMode }, caution: t('audio-onsets.sbb8e524') };
      }
      const peaks = selectOnsetPeaks(raw, settings), first = Math.max(0, Math.ceil((settings.startSeconds - raw.startSeconds) * raw.frameRate)), last = Math.min(raw.salience.length, Math.ceil((settings.endSeconds - raw.startSeconds) * raw.frameRate));
      return { peaks, estimatedBpms: estimateOnsetBpms(raw.salience.subarray(first, last), raw.frameRate), sampleRate: buffer.sampleRate, startSeconds: settings.startSeconds, endSeconds: settings.endSeconds, detector: 'ddc', method: 'ddc-onset-cnn', focus: 'all', threshold: settings.minConfidence, minGap: settings.minGap, cacheHit, model: { version: raw.version, revision: raw.revision, frameRate: raw.frameRate, device: raw.device }, statistics: { frames: last - first, selected: peaks.length, channelMode: raw.channelMode }, caution: t('audio-onsets.s8504ccf') };
    } finally { signal?.removeEventListener('abort', cancel); this.busy = false; this.progress = null; this.controller = null; this.changed(); }
  }
}
