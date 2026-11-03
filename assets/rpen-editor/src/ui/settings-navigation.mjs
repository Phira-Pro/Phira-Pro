import { t } from '../i18n.mjs';

const categories = ['general', 'appearance', 'editor', 'preview', 'audio', 'input', 'storage', 'compatibility', 'about'];
const icons = {
  general: 'M4 6h16M4 12h16M4 18h16M8 3v6M16 9v6M10 15v6',
  appearance: 'M12 3a9 9 0 1 0 0 18h1a2 2 0 0 0 1-3.7 1.5 1.5 0 0 1 1-2.8h2A4 4 0 0 0 21 10c0-4-4-7-9-7ZM7 10h.01M10 6h.01M15 6h.01M18 10h.01',
  editor: 'M4 3v18h17M8 7h4v5H8zM15 5h4v9h-4zM8 16h11',
  preview: 'M3 4h18v13H3zM8 21h8M12 17v4M10 8l5 3-5 3z',
  audio: 'M9 18V5l11-2v13M9 8l11-2M9 18a3 3 0 1 1-3-3c1.7 0 3 1.3 3 3ZM20 16a3 3 0 1 1-3-3c1.7 0 3 1.3 3 3Z',
  input: 'M3 5h18v14H3zM6 9h.01M10 9h.01M14 9h.01M18 9h.01M6 13h.01M10 13h.01M14 13h.01M18 13h.01M8 16h8',
  storage: 'M4 3h13l3 3v15H4zM8 3v6h8V3M8 21v-8h8v8',
  compatibility: 'M8 3v5M16 3v5M5 8h14v4a7 7 0 0 1-14 0ZM12 19v3',
  about: 'M12 8h.01M11 12h1v5M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z',
};
const normalize = value => String(value).normalize('NFKC').toLocaleLowerCase().replace(/\s+/g, '');

export class SettingsNavigation {
  constructor(dialog, body) {
    Object.assign(this, { dialog, body });
    this.groups = new Map(); this.openDetails = new Map(); this.active = null;
    this.layout = document.createElement('div'); this.layout.className = 'settings-layout';
    this.nav = document.createElement('nav'); this.nav.className = 'settings-nav'; this.nav.setAttribute('aria-label', t('settings.navigation'));
    this.empty = document.createElement('p'); this.empty.className = 'settings-empty'; this.empty.textContent = t('settings.noResults'); this.empty.hidden = true; this.empty.setAttribute('role', 'status');
    body.id = 'settings-content'; body.tabIndex = 0; body.setAttribute('aria-label', t('settings.content'));
    body.before(this.layout); this.layout.append(this.nav, body);
    for (const category of categories) {
      const section = document.createElement('section'); section.className = 'settings-category'; section.dataset.settingsSection = category; section.id = 'settings-category-' + category;
      const heading = document.createElement('h2'); heading.className = 'settings-category-title'; heading.id = section.id + '-title'; heading.textContent = t('settings.category.' + category); section.setAttribute('aria-labelledby', heading.id);
      const content = document.createElement('div'); content.className = 'settings-category-content'; section.append(heading, content); body.append(section);
      const link = document.createElement('a'); link.href = '#' + section.id; link.className = 'settings-nav-item'; link.dataset.settingsTarget = category;
      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); svg.setAttribute('viewBox', '0 0 24 24'); svg.setAttribute('aria-hidden', 'true');
      const path = document.createElementNS(svg.namespaceURI, 'path'); path.setAttribute('d', icons[category]); svg.append(path);
      const label = document.createElement('span'); label.textContent = heading.textContent; link.append(svg, label); link.setAttribute('aria-controls', section.id);
      link.onclick = event => { event.preventDefault(); this.go(category); };
      this.nav.append(link); this.groups.set(category, { section, content, link });
    }
    body.append(this.empty);
    this.collect();
    this.observer = new MutationObserver(records => {
      if (records.some(record => record.target === body && [...record.addedNodes].some(node => node.nodeType === 1 && !node.classList.contains('settings-category') && node !== this.empty))) this.collect();
      else if (this.query) this.filter(this.query);
    });
    this.observer.observe(body, { childList: true, subtree: true, characterData: true });
    this.resize = new ResizeObserver(() => this.schedule()); this.resize.observe(body);
    for (const { section } of this.groups.values()) this.resize.observe(section);
    body.addEventListener('scroll', () => this.schedule(), { passive: true });
    for (const type of ['wheel', 'pointerdown', 'keydown']) body.addEventListener(type, () => { this.destination = null; }, { passive: true });
    this.nav.addEventListener('keydown', event => {
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
      const links = [...this.nav.children].filter(link => !link.hidden), current = links.indexOf(event.target);
      if (current < 0) return;
      event.preventDefault();
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? links.length - 1 : Math.max(0, Math.min(links.length - 1, current + (event.key === 'ArrowDown' ? 1 : -1)));
      links[next].focus(); this.go(links[next].dataset.settingsTarget);
    });
    dialog.addEventListener('toggle', () => { if (dialog.open) this.schedule(); });
    dialog.addEventListener('beforetoggle', event => {
      if (event.newState === 'closed') this.reopenCategory = this.active;
      else requestAnimationFrame(() => { if (this.reopenCategory) this.go(this.reopenCategory, false); this.schedule(); });
    });
    dialog.addEventListener('close', () => { this.destination = null; });
    document.addEventListener('rpen-editor-skin-changed', () => this.schedule());
  }
  collect() {
    for (const node of [...this.body.children]) {
      if (node.classList.contains('settings-category') || node === this.empty) continue;
      const category = node.dataset.settingsCategory ?? (node.id === 'storage-settings' ? 'storage' : 'general');
      node.classList.add('settings-card'); (this.groups.get(category) ?? this.groups.get('general')).content.append(node);
    }
    this.filter(this.query ?? '');
  }
  filter(query) {
    this.query = query; const wanted = normalize(query); let matches = 0;
    for (const { section, content, link } of this.groups.values()) {
      let visible = false;
      for (const card of content.children) {
        const labels = [...card.querySelectorAll('[aria-label]')].map(node => node.getAttribute('aria-label')).join(' ');
        const found = !wanted || normalize(card.textContent + labels + section.firstElementChild.textContent).includes(wanted);
        card.classList.toggle('settings-search-hidden', !found); visible ||= found;
        if (found) matches++;
        for (const details of [...(card.matches('details') ? [card] : []), ...card.querySelectorAll('details')]) {
          if (wanted && found) { if (!this.openDetails.has(details)) this.openDetails.set(details, details.open); details.open = true; }
          else if (this.openDetails.has(details)) { details.open = this.openDetails.get(details); this.openDetails.delete(details); }
        }
      }
      section.hidden = !visible; link.hidden = !visible;
    }
    this.empty.hidden = matches > 0;
    if (wanted !== this.lastFilter) { this.destination = null; this.body.scrollTop = 0; this.lastFilter = wanted; }
    this.schedule();
  }
  go(category, animate = false) {
    const group = this.groups.get(category); if (!group || group.section.hidden) return;
    const bounds = this.body.getBoundingClientRect(), target = group.section.getBoundingClientRect();
    const top = Math.max(0, Math.min(this.body.scrollHeight - this.body.clientHeight, this.body.scrollTop + target.top - bounds.top - 24));
    const smooth = animate && !matchMedia('(prefers-reduced-motion: reduce)').matches;
    this.destination = smooth ? { category, top, until: performance.now() + 1200 } : null; this.activate(category);
    this.body.scrollTo({ top, behavior: smooth ? 'smooth' : 'instant' });
  }
  activate(category) {
    if (this.active === category) return;
    this.active = category;
    for (const [key, { link }] of this.groups) {
      if (key === category) link.setAttribute('aria-current', 'location'); else link.removeAttribute('aria-current');
    }
    const link = this.groups.get(category)?.link;
    if (link && (link.offsetTop < this.nav.scrollTop || link.offsetTop + link.offsetHeight > this.nav.scrollTop + this.nav.clientHeight)) this.nav.scrollTop = link.offsetTop - this.nav.clientHeight / 2;
  }
  schedule() {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => { this.frame = null; this.update(); });
  }
  update() {
    if (!this.dialog.open) return;
    if (this.destination) {
      if (performance.now() < this.destination.until && Math.abs(this.body.scrollTop - this.destination.top) > 2) return;
      this.destination = null;
    }
    const visible = [...this.groups].filter(([, group]) => !group.section.hidden);
    if (!visible.length) { this.activate(null); return; }
    const top = this.body.getBoundingClientRect().top + 90;
    let active = visible[0][0];
    for (const [category, { section }] of visible) if (section.getBoundingClientRect().top <= top) active = category;
    if (this.body.scrollHeight > this.body.clientHeight && this.body.scrollTop + this.body.clientHeight >= this.body.scrollHeight - 2) active = visible.at(-1)[0];
    this.activate(active);
  }
}
