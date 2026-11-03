import { toCanvas } from '../core/vendor/html-to-image.mjs';
import { checkAgentSignal } from '../core/agent-commands.mjs';
import { t } from '../i18n.mjs';

const privateSelector = '#ai-panel,#ai-config,#ai-tab,#ai-tool,[data-agent-private],input[type=password],input[autocomplete=current-password],input[autocomplete=new-password]';

export function agentCaptureRect(bounds, viewport) {
  const x = Math.max(0, Math.floor(bounds.x)), y = Math.max(0, Math.floor(bounds.y));
  const right = Math.min(viewport.width, Math.ceil(bounds.x + bounds.width)), bottom = Math.min(viewport.height, Math.ceil(bounds.y + bounds.height));
  if (right <= x || bottom <= y) throw Error(t('agent-capture.sc947046'));
  return { x, y, width: right - x, height: bottom - y };
}

export class AgentCapture {
  constructor({ controls, refresh = () => {}, bridge = globalThis.rpenAgentCapture } = {}) { Object.assign(this, { controls, refresh, bridge }); }
  target(name, handle) {
    let node;
    if (name === 'editor') node = document.body;
    else if (name === 'timeline') node = document.querySelector('main.editor');
    else if (name === 'panel') node = document.querySelector('aside.inspector');
    else if (name === 'dialog') node = [...document.querySelectorAll('dialog[open]')].filter(entry => !entry.closest(privateSelector)).at(-1);
    else if (name === 'control') { node = this.controls?.handles.get(handle); if (!this.controls?.allowed(node)) throw Error(t('agent-capture.sbc8b197')); }
    if (!node || !node.isConnected || node.closest(privateSelector) || !node.getClientRects().length) throw Error(t('agent-capture.s49ac78e'));
    return node;
  }
  async capture({ target = 'editor', handle } = {}, signal) {
    checkAgentSignal(signal);
    let timer, abort;
    try {
      return await new Promise((resolve, reject) => {
        abort = () => reject(signal.reason instanceof Error ? signal.reason : new DOMException(t('agent-capture.sd070680'), 'AbortError'));
        signal?.addEventListener('abort', abort, { once: true }); timer = setTimeout(() => reject(Error(t('agent-capture.s9165bd3'))), 20000);
        this.captureImage({ target, handle }, signal).then(resolve, reject);
      });
    } catch (error) { if (error instanceof Error) throw error; throw Error(t('agent-capture.s44ac3bc')); }
    finally { clearTimeout(timer); signal?.removeEventListener('abort', abort); }
  }
  async captureImage({ target, handle }, signal) {
    checkAgentSignal(signal);
    const node = this.target(target, handle), viewport = { width: innerWidth, height: innerHeight };
    const rect = target === 'editor' ? { x: 0, y: 0, ...viewport } : agentCaptureRect(node.getBoundingClientRect(), viewport);
    this.refresh(); await new Promise(resolve => requestAnimationFrame(resolve)); checkAgentSignal(signal);
    const redactions = [...document.querySelectorAll(privateSelector)].filter(entry => entry.getClientRects().length && getComputedStyle(entry).visibility !== 'hidden').map(entry => entry.getBoundingClientRect());
    let canvas;
    if (this.bridge) {
      const result = await this.bridge.capture(rect); checkAgentSignal(signal);
      if (!result.ok) throw Error(result.message);
      const image = new Image(); image.src = result.image; await image.decode();
      canvas = document.createElement('canvas'); canvas.width = image.naturalWidth; canvas.height = image.naturalHeight; canvas.getContext('2d').drawImage(image, 0, 0);
    } else {
      const root = document.body;
      const full = await toCanvas(root, { width: viewport.width, height: viewport.height, pixelRatio: Math.min(1, 1600 / viewport.width), backgroundColor: getComputedStyle(root).backgroundColor || '#20232a',
        filter: entry => !(entry instanceof Element) || !entry.closest(privateSelector + ',[hidden]') && !['SCRIPT', 'LINK', 'TEMPLATE'].includes(entry.tagName) && getComputedStyle(entry).display !== 'none' && getComputedStyle(entry).visibility !== 'hidden', style: { margin: '0', overflow: 'hidden' }, preferredFontFormat: 'woff2' });
      checkAgentSignal(signal);
      const scale = full.width / viewport.width;
      canvas = document.createElement('canvas'); canvas.width = Math.max(1, Math.round(rect.width * scale)); canvas.height = Math.max(1, Math.round(rect.height * scale));
      canvas.getContext('2d').drawImage(full, rect.x * scale, rect.y * scale, rect.width * scale, rect.height * scale, 0, 0, canvas.width, canvas.height);
    }
    redactions.push(...[...document.querySelectorAll(privateSelector)].filter(entry => entry.getClientRects().length).map(entry => entry.getBoundingClientRect()));
    const context = canvas.getContext('2d'), scaleX = canvas.width / rect.width, scaleY = canvas.height / rect.height;
    context.fillStyle = '#28333f';
    for (const bounds of redactions) context.fillRect((bounds.x - rect.x - 2) * scaleX, (bounds.y - rect.y - 2) * scaleY, (bounds.width + 4) * scaleX, (bounds.height + 4) * scaleY);
    checkAgentSignal(signal);
    return { image: canvas.toDataURL('image/png'), width: canvas.width, height: canvas.height, target, redacted: true, mode: this.bridge ? 'native' : 'dom' };
  }
}
