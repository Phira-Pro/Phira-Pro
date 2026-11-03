import { t } from '../i18n.mjs';
export class ResourceCache {
  constructor({ maxEntries, maxBytes, dispose = () => {}, retain = () => false }) {
    this.maxEntries = maxEntries; this.maxBytes = maxBytes; this.dispose = dispose; this.retain = retain; this.items = new Map(); this.bytes = 0;
  }
  get size() { return this.items.size; }
  has(key) { return this.items.has(key); }
  peek(key) { return this.items.get(key)?.value; }
  get(key) { const entry = this.items.get(key); if (!entry) return; this.items.delete(key); this.items.set(key, entry); return entry.value; }
  set(key, value, bytes) {
    if (!Number.isFinite(bytes) || bytes < 0) throw Error(t('resource-cache.sf43b974'));
    const previous = this.items.get(key); if (previous && previous.value === value) { this.bytes += bytes - previous.bytes; this.items.delete(key); }
    else this.delete(key);
    this.items.set(key, { value, bytes }); this.bytes += previous && previous.value === value ? 0 : bytes; this.trim(); return value;
  }
  delete(key) { const entry = this.items.get(key); if (!entry) return; this.items.delete(key); this.bytes -= entry.bytes; this.dispose(entry.value); }
  trim() {
    for (const [key, entry] of this.items) {
      if (this.items.size <= this.maxEntries && this.bytes <= this.maxBytes) break;
      if (!this.retain(entry.value)) this.delete(key);
    }
  }
  clear() { for (const key of [...this.items.keys()]) this.delete(key); }
  keys() { return this.items.keys(); }
}
