import { AUDIO_EXTENSION } from '../core/asset-library.mjs';
import { mediaType } from './files.mjs';

export function formatAssetDuration(seconds) {
  if (!Number.isFinite(seconds) || seconds < 0) return '';
  const hundredths = Math.round(seconds * 100), totalSeconds = Math.floor(hundredths / 100), minutes = Math.floor(totalSeconds / 60), hours = Math.floor(minutes / 60);
  return (hours ? hours + ':' + String(minutes % 60).padStart(2, '0') : String(minutes)) + ':' + String(totalSeconds % 60).padStart(2, '0') + '.' + String(hundredths % 100).padStart(2, '0');
}

export function probeAssetMetadata(bytes, name, signal) {
  return new Promise(resolve => {
    if (signal.aborted) { resolve(null); return; }
    const isAudio = AUDIO_EXTENSION.test(name), element = document.createElement(isAudio ? 'audio' : 'img');
    const url = URL.createObjectURL(new Blob([bytes], { type: mediaType(name) }));
    let settled = false;
    const finish = value => {
      if (settled) return; settled = true; clearTimeout(timer); signal.removeEventListener('abort', abort);
      element.onload = element.onloadedmetadata = element.onerror = null;
      if (isAudio) element.pause(); element.removeAttribute('src'); if (isAudio) element.load();
      URL.revokeObjectURL(url); resolve(value);
    };
    const abort = () => finish(null), timer = setTimeout(abort, 15000);
    signal.addEventListener('abort', abort, { once: true }); element.onerror = abort;
    if (isAudio) {
      element.preload = 'metadata'; element.onloadedmetadata = () => finish(Number.isFinite(element.duration) && element.duration >= 0 ? { kind: 'audio', duration: element.duration } : null);
    } else element.onload = () => finish(element.naturalWidth > 0 && element.naturalHeight > 0 ? { kind: 'image', width: element.naturalWidth, height: element.naturalHeight } : null);
    element.src = url;
  });
}

export class AssetMetadataReader {
  constructor({ probe = probeAssetMetadata, concurrency = 2 } = {}) {
    this.probe = probe; this.concurrency = Math.max(1, concurrency); this.cache = new WeakMap(); this.pending = new Map(); this.queue = []; this.active = 0;
  }
  read(bytes, name) {
    if (this.cache.has(bytes)) return Promise.resolve(this.cache.get(bytes));
    if (this.pending.has(bytes)) return this.pending.get(bytes).promise;
    const controller = new AbortController(); let resolve;
    const promise = new Promise(finish => { resolve = finish; }), job = { bytes, name, controller, promise, resolve };
    this.pending.set(bytes, job); this.queue.push(job); this.drain(); return promise;
  }
  cancel() {
    const pending = [...this.pending.values()]; this.pending.clear(); this.queue.length = 0;
    for (const job of pending) { job.controller.abort(); job.resolve(null); }
  }
  drain() {
    while (this.active < this.concurrency && this.queue.length) {
      const job = this.queue.shift(); if (job.controller.signal.aborted) continue;
      this.active++;
      Promise.resolve().then(() => job.controller.signal.aborted ? null : this.probe(job.bytes, job.name, job.controller.signal)).catch(() => null).then(value => {
        if (!job.controller.signal.aborted) this.cache.set(job.bytes, value); job.resolve(value);
      }).finally(() => { this.active--; if (this.pending.get(job.bytes) === job) this.pending.delete(job.bytes); this.drain(); });
    }
  }
}
