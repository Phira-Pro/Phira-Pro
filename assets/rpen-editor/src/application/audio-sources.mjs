import { LocalAudioSeparator } from '../platform/audio-separation.mjs';
import { mixAudioSources } from '../platform/audio-mix.mjs';
import { STEM_NAMES, SEPARATION_MODELS, audioSourceColor, sourceAudioRange } from '../core/audio-sources.mjs';
import { checkAgentSignal, agentAbort } from '../core/agent-commands.mjs';
import { ResourceCache } from '../core/resource-cache.mjs';
import { t } from '../i18n.mjs';

export class AudioSources extends EventTarget {
  constructor(getAudio, { separator = new LocalAudioSeparator(), mixCacheBytes = 192 * 1024 ** 2, mixCacheEntries = 4 } = {}) {
    super(); this.getAudio = getAudio; this.separator = separator; this.entries = new Map(); this.mixCache = new ResourceCache({ maxBytes: mixCacheBytes, maxEntries: mixCacheEntries }); this.revision = 0; this.serial = 0; this.selectionSerial = 0; this.reset(getAudio().buffer);
  }
  changed() { this.dispatchEvent(new Event('change')); }
  reset(buffer) {
    this.controller?.abort(); this.selectionController?.abort(); this.selectionSerial++; this.getAudio().resetPlaybackBuffer?.({ clearCache: true }); this.mixCache.clear(); this.original = buffer ?? null; this.revision++;
    this.colorSerial = 1; this.entries.clear(); this.entries.set('original', { id: 'original', name: STEM_NAMES.original, buffer: this.original, offsetSeconds: 0, kind: 'original', color: audioSourceColor(0) });
    this.selection = new Set(['original']); this.pendingSelection = null; this.mixEntry = null; this.selecting = false; this.changed();
  }
  sync() { if ((this.getAudio().buffer ?? null) !== this.original) this.reset(this.getAudio().buffer); }
  get selected() { return this.selection.size === 1 ? [...this.selection][0] : 'selection'; }
  get selectedIds() { return [...this.selection]; }
  list() { this.sync(); const selected = this.pendingSelection ?? this.selection; return [...this.entries.values()].map(({ id, name, kind, model, buffer, duration, color }) => ({ id, name, kind, model, color, offsetSeconds: 0, duration: buffer?.duration ?? duration, selected: selected.has(id) })); }
  entry(id = this.selected) {
    this.sync(); if (id === 'selection' && this.mixEntry) return this.mixEntry; if (this.entries.has(id)) return this.entries.get(id);
    const matches = [...this.entries.values()].filter(entry => entry.stem === id || entry.name === id); if (matches.length !== 1) throw Error(t('audio-sources.sffa2983')); return matches[0];
  }
  isCurrent(entry) { if (entry?.originalEntry) return this.isCurrent(entry.originalEntry); return entry?.members ? entry.members.every(member => this.entries.get(member.id) === member) && entry.buffer === this.mixEntry?.buffer : this.entries.get(entry?.id) === entry; }
  async resolve(id = this.selected, signal) {
    checkAgentSignal(signal); const entry = this.entry(id), original = this.original;
    if (!entry.buffer) { const cached = this.mixCache.get(JSON.stringify([entry.id])); if (cached?.direct && cached.original === original && cached.members[0] === entry) entry.buffer = cached.buffer; }
    if (!entry.buffer && entry.result) {
      if (!entry.loading) entry.loading = (async () => { const bytes = await this.separator.bytes(entry.result, entry.stem); return this.getAudio().ensureContext().decodeAudioData(bytes.buffer); })().finally(() => { entry.loading = null; });
      const buffer = await entry.loading; checkAgentSignal(signal);
      if (original !== this.original || this.entries.get(entry.id) !== entry) throw Error(t('audio-sources.sda4e35e')); entry.buffer = buffer;
    }
    if (!entry.buffer) throw Error(t('audio-sources.s10cfdf7')); return entry;
  }
  async resolveSelection(signal) {
    this.sync(); checkAgentSignal(signal); if (this.mixEntry && this.mixEntry.ids.join('\0') === this.selectedIds.join('\0')) return this.mixEntry;
    return this.resolve(this.selected, signal);
  }
  async resolveForAnalysis(id, signal) {
    const entry = id === undefined || id === 'selection' ? await this.resolveSelection(signal) : await this.resolve(id, signal);
    if (entry.buffer.length === this.original?.length && entry.buffer.sampleRate === this.original.sampleRate) return entry;
    if (!entry.aligned || entry.aligned.original !== this.original) { const result = await mixAudioSources([entry], this.original, this.getAudio().ensureContext(), signal); entry.aligned = { original: this.original, buffer: result.buffer }; }
    return { ...entry, buffer: entry.aligned.buffer, originalEntry: entry, offsetSeconds: 0 };
  }
  async select(ids) {
    this.sync(); const requested = [...new Set((Array.isArray(ids) ? ids : [ids]).map(id => this.entry(id).id))];
    if (!requested.length) throw Error(t('audio-sources.s6b701a3')); if (requested.includes('selection')) throw Error(t('audio-sources.sf133147'));
    this.selectionController?.abort(); const controller = this.selectionController = new AbortController(), serial = ++this.selectionSerial, original = this.original;
    this.pendingSelection = new Set(requested); this.selecting = true; this.changed();
    try {
      const key = JSON.stringify([...requested].sort()), cached = this.mixCache.get(key), reusable = cached && cached.original === original && cached.members.every(member => this.entries.get(member.id) === member);
      const members = [];
      for (const id of requested) members.push(reusable ? this.entry(id) : await this.resolve(id, controller.signal));
      const unchanged = this.mixEntry && this.mixEntry.ids.join('\0') === requested.join('\0') && this.mixEntry.members.every((entry, index) => entry === members[index]);
      let entry;
      if (reusable) entry = { ...cached, id: requested.length === 1 ? requested[0] : 'selection', ids: requested, members, name: members.map(member => member.name).join(' + ') };
      else if (unchanged) entry = this.mixEntry;
      else if (requested.length === 1 && members[0].buffer.length === original.length && members[0].buffer.sampleRate === original.sampleRate) entry = { ...members[0], ids: requested, members };
      else { const result = await mixAudioSources(members, original, this.getAudio().ensureContext(), controller.signal); entry = { id: requested.length === 1 ? requested[0] : 'selection', name: members.map(member => member.name).join(' + '), kind: 'mix', offsetSeconds: 0, ids: requested, members, ...result }; }
      checkAgentSignal(controller.signal); if (serial !== this.selectionSerial || original !== this.original || !members.every(member => this.entries.get(member.id) === member)) throw agentAbort();
      if (!(requested.length === 1 && requested[0] === 'original')) this.mixCache.set(key, { ...entry, original, direct: reusable ? cached.direct : members.length === 1 && entry.buffer === members[0].buffer }, entry.buffer.length * entry.buffer.numberOfChannels * 4);
      if (requested.length === 1 && requested[0] === 'original') this.getAudio().resetPlaybackBuffer?.();
      else await this.getAudio().setPlaybackBuffer?.(entry.buffer, { signal: controller.signal });
      checkAgentSignal(controller.signal); if (serial !== this.selectionSerial || original !== this.original) throw agentAbort();
      this.selection = new Set(requested); this.mixEntry = entry; this.revision++; this.trimDecoded(); return entry;
    } finally { if (serial === this.selectionSerial) { this.pendingSelection = null; this.selecting = false; this.selectionController = null; this.changed(); } }
  }
  async setSelected(id, selected) {
    const next = new Set(this.pendingSelection ?? this.selection); if (selected) next.add(this.entry(id).id); else next.delete(this.entry(id).id); return this.select([...next]);
  }
  trimDecoded() {
    const unused = [...this.entries.values()].filter(entry => entry.result && entry.buffer && !this.selection.has(entry.id)); while (unused.length > 1) { const entry = unused.shift(); entry.buffer = null; entry.aligned = null; }
  }
  invalidateMixes(ids) { for (const key of [...this.mixCache.keys()]) if (this.mixCache.peek(key).members.some(member => ids.has(member.id))) this.mixCache.delete(key); }
  async separate(options = {}, signal) {
    this.sync(); if (!this.original) throw Error(t('audio-sources.s510310b')); if (this.busy) throw Error(t('audio-sources.sbf382b3'));
    const model = options.model ?? 'htdemucs'; if (!SEPARATION_MODELS.some(entry => entry.id === model)) throw Error(t('audio-sources.s785d1e0'));
    const controller = this.controller = new AbortController(), cancel = () => controller.abort(); signal?.addEventListener('abort', cancel, { once: true }); if (signal?.aborted) cancel();
    const original = this.original; this.busy = true; this.progress = { message: t('audio-sources.s16dcb96') }; this.changed();
    try {
      const result = await this.separator.separate(original, options, controller.signal, progress => { this.progress = progress; this.dispatchEvent(new Event('progress')); });
      if (original !== this.original || this.getAudio().buffer !== original) throw Error(t('audio-sources.sad5759d'));
      let refresh = false;
      this.invalidateMixes(new Set(result.sources.map(source => model + '/' + source.id)));
      for (const source of result.sources) {
        const id = model + '/' + source.id; refresh ||= this.selection.has(id);
        const color = this.entries.get(id)?.color ?? audioSourceColor(this.colorSerial++);
        this.entries.set(id, { id, stem: source.id, name: t('audio-sources.s282e37a', { p0: STEM_NAMES[source.id] ?? source.id, p1: model === 'htdemucs_6s' ? t('audio-sources.sd1d87ef') : t('audio-sources.se977b25') }), kind: 'separated', model, color, offsetSeconds: 0, duration: source.frames / source.sampleRate, buffer: null, result });
      }
      this.revision++; if (refresh) await this.select(this.selectedIds);
      return { model, sources: this.list().filter(entry => entry.model === model), cacheHit: result.cacheHit === true };
    } finally { signal?.removeEventListener('abort', cancel); if (this.controller === controller) { this.busy = false; this.progress = null; this.controller = null; this.changed(); } }
  }
  async import(bytes, name) {
    this.sync(); if (!this.original) throw Error(t('audio-sources.s2a329d5'));
    if (bytes.byteLength > 512 * 1024 ** 2 || this.entries.size >= 33) throw Error(t('audio-sources.se25d846'));
    const original = this.original, buffer = await this.getAudio().ensureContext().decodeAudioData(bytes instanceof ArrayBuffer ? bytes.slice(0) : bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
    if (original !== this.original) throw Error(t('audio-sources.sc28dea8'));
    const id = 'imported/' + ++this.serial; this.entries.set(id, { id, name, kind: 'imported', buffer, color: audioSourceColor(this.colorSerial++), offsetSeconds: 0 }); this.revision++; this.changed(); return id;
  }
  async remove(id) {
    const entry = this.entry(id); if (entry.kind === 'original') return;
    if ((this.pendingSelection ?? this.selection).has(entry.id)) { const next = this.selectedIds.filter(selected => selected !== entry.id); await this.select(next.length ? next : ['original']); }
    if (this.entries.get(entry.id) !== entry) return; this.invalidateMixes(new Set([entry.id])); this.entries.delete(entry.id); this.revision++; this.changed();
  }
  range(source, start, end) { return sourceAudioRange(source, start, end); }
}
