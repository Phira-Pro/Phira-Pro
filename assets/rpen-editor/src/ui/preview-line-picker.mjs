import { t } from '../i18n.mjs';

export class PreviewLinePicker {
  constructor() {
    this.host = document.createElement('div'); this.host.className = 'preview-line-picker'; this.host.hidden = true; this.host.setAttribute('role', 'menu'); this.host.setAttribute('aria-label', t('preview-lines.choose'));
    document.body.append(this.host);
    this.host.addEventListener('pointerdown', event => event.stopPropagation());
    this.host.addEventListener('wheel', event => event.stopPropagation());
    window.addEventListener('pointerdown', event => { if (!this.host.contains(event.target)) this.hide(); }, true);
    window.addEventListener('blur', () => this.hide()); window.addEventListener('resize', () => this.hide());
    this.host.addEventListener('keydown', event => {
      const buttons = [...this.host.querySelectorAll('button')], index = buttons.indexOf(document.activeElement);
      if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault(); event.stopPropagation(); buttons[event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length]?.focus();
      }
    });
  }
  get active() { return !this.host.hidden; }
  hide() { this.host.hidden = true; this.host.replaceChildren(); }
  show(matches, event, select) {
    this.hide();
    for (const guide of matches) {
      const button = document.createElement('button'); button.type = 'button'; button.setAttribute('role', 'menuitem'); button.dataset.lineIndex = guide.index;
      const number = document.createElement('span'), alpha = document.createElement('span'); number.textContent = '#' + guide.index; alpha.textContent = String(Number(guide.alpha.toFixed(3)));
      button.append(number, alpha); button.onclick = () => { this.hide(); select(guide.index); }; this.host.append(button);
    }
    this.host.hidden = false;
    const rect = this.host.getBoundingClientRect();
    this.host.style.left = Math.max(4, Math.min(window.innerWidth - rect.width - 4, event.clientX + 8)) + 'px';
    this.host.style.top = Math.max(4, Math.min(window.innerHeight - rect.height - 4, event.clientY + 8)) + 'px';
    this.host.firstElementChild?.focus({ preventScroll: true });
  }
}
