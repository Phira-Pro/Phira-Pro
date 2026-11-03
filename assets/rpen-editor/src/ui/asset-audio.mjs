import { mediaType } from '../platform/files.mjs';
import { t } from '../i18n.mjs';

export class AssetAudioPlayer {
  constructor({ volume = () => 0.3, reportError = () => {} } = {}) {
    this.volume = volume; this.reportError = reportError; this.revision = 0;
    this.element = document.createElement('audio'); this.element.controls = true; this.element.preload = 'none'; this.element.className = 'asset-audio-preview'; this.element.hidden = true;
  }
  async play(bytes, name) {
    this.stop(); const revision = this.revision;
    this.url = URL.createObjectURL(new Blob([bytes], { type: mediaType(name) }));
    this.element.src = this.url; this.element.hidden = false; this.element.setAttribute('aria-label', t('asset-audio.preview', { name }));
    this.element.volume = Math.max(0, Math.min(1, Number(this.volume()) || 0));
    try { await this.element.play(); }
    catch (error) { if (revision === this.revision) this.reportError(Error(t('asset-audio.failed', { name, reason: error.message }))); }
  }
  stop() {
    this.revision++; this.element.pause(); this.element.removeAttribute('src'); this.element.load(); this.element.hidden = true;
    if (this.url) URL.revokeObjectURL(this.url); this.url = null;
  }
}
