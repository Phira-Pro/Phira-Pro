const NS = 'http://www.w3.org/2000/svg';
const node = (name, attrs = {}) => { const value = document.createElementNS(NS, name); for (const [key, text] of Object.entries(attrs)) value.setAttribute(key, text); return value; };

export function clippedDiagnosticRect(rect, clip, padding = 5) {
  if (![rect.x, rect.y, rect.width, rect.height, clip.x, clip.y, clip.width, clip.height].every(Number.isFinite)) return null;
  const left = Math.max(clip.x + 1, rect.x - padding), top = Math.max(clip.y + 1, rect.y - padding);
  const right = Math.min(clip.x + clip.width - 1, rect.x + Math.max(1, rect.width) + padding), bottom = Math.min(clip.y + clip.height - 1, rect.y + Math.max(1, rect.height) + padding);
  return right > left && bottom > top ? { x: left, y: top, width: right - left, height: bottom - top } : null;
}

// Animate only a small SVG. The editor canvases do not need to repaint just
// to move this outline; only the focused object's bounds are read.
export class DiagnosticHighlight {
  constructor(stage, getContext, duration = 4500) {
    this.stage = stage; this.getContext = getContext; this.duration = duration; this.rectangles = [];
    this.svg = node('svg', { 'aria-hidden': 'true', class: 'diagnostic-highlight' }); this.svg.style.display = 'none'; stage.append(this.svg);
  }
  show(issue, geometry) {
    this.clear(); const context = this.getContext(); this.target = { issue, geometry, owner: context.session, chart: context.session.chart }; this.until = performance.now() + this.duration;
    this.svg.style.display = ''; this.update();
  }
  clear() { cancelAnimationFrame(this.frame); this.target = null; this.svg.style.display = 'none'; this.svg.replaceChildren(); this.rectangles = []; }
  update() {
    if (!this.target) return;
    const context = this.getContext();
    if (performance.now() >= this.until || context.home || context.session !== this.target.owner || context.session.chart !== this.target.chart) { this.clear(); return; }
    const stage = this.stage.getBoundingClientRect(), viewBox = `0 0 ${stage.width} ${stage.height}`;
    if (this.svg.getAttribute('viewBox') !== viewBox) this.svg.setAttribute('viewBox', viewBox);
    const rectangles = []; let entries;
    try { entries = this.target.geometry(this.target.issue) ?? []; } catch { entries = []; }
    for (const entry of entries) {
      const canvas = entry.canvas, bounds = canvas.getBoundingClientRect(); if (!(bounds.width > 0 && bounds.height > 0) || canvas.hidden || getComputedStyle(canvas).visibility === 'hidden') continue;
      const project = entry.project ?? ((x, y) => ({ x: bounds.left + x * bounds.width / canvas.clientWidth, y: bounds.top + y * bounds.height / canvas.clientHeight }));
      const corners = [[entry.x, entry.y], [entry.x + entry.width, entry.y], [entry.x, entry.y + entry.height], [entry.x + entry.width, entry.y + entry.height]].map(([x, y]) => project(x, y));
      const xs = corners.map(point => point.x - stage.left), ys = corners.map(point => point.y - stage.top);
      let left = Math.max(stage.left, bounds.left), top = Math.max(stage.top, bounds.top), right = Math.min(stage.right, bounds.right), bottom = Math.min(stage.bottom, bounds.bottom);
      // Scroll containers, notably multi-column global events, clip the canvas.
      for (let parent = canvas.parentElement; parent && parent !== this.stage; parent = parent.parentElement) {
        const style = getComputedStyle(parent); if (!/(hidden|auto|scroll|clip)/.test(style.overflowX + style.overflowY)) continue;
        const box = parent.getBoundingClientRect(); left = Math.max(left, box.left); top = Math.max(top, box.top); right = Math.min(right, box.right); bottom = Math.min(bottom, box.bottom);
      }
      const rect = clippedDiagnosticRect({ x: Math.min(...xs), y: Math.min(...ys), width: Math.max(...xs) - Math.min(...xs), height: Math.max(...ys) - Math.min(...ys) }, { x: left - stage.left, y: top - stage.top, width: right - left, height: bottom - top });
      if (rect) rectangles.push(rect);
    }
    this.rectangles = rectangles;
    while (this.svg.children.length > rectangles.length) this.svg.lastChild.remove();
    rectangles.forEach((rect, index) => {
      let group = this.svg.children[index];
      if (!group) { group = node('g'); group.append(node('rect', { class: 'diagnostic-highlight-shadow', rx: 5 }), node('rect', { class: 'diagnostic-highlight-outline', rx: 5 })); this.svg.append(group); }
      for (const element of group.children) for (const [key, value] of Object.entries(rect)) if (element.getAttribute(key) !== String(value)) element.setAttribute(key, value);
      group.lastChild.style.strokeDashoffset = String(-(performance.now() / 45 % 24));
    });
    this.frame = requestAnimationFrame(() => this.update());
  }
}
