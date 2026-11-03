import { EDITOR_SKIN_CANVASES } from '../core/editor-skin.mjs';

let settings = null; let revision = 0;
const contexts = new WeakMap();
const originalContexts = new WeakMap();
export const originalEditorContext = context => originalContexts.get(context) ?? context;
const palette = value => Object.fromEntries(Object.entries(value ?? {}).map(([key, color]) => [key.toLowerCase(), color]));
export function setEditorCanvasSkin(value, font = '', fonts = new Map()) {
  settings = value ? { ...value, palette: palette(value.palette), targets: Object.fromEntries(Object.entries(value.targets ?? {}).map(([role, config]) => [role, { ...config, palette: palette(config.palette), resolvedFont: fonts.get(config.font) || font }])), resolvedFont: font } : null;
  revision++;
}
export function editorCanvasRevision() { return revision; }
function canvasRole(canvas) {
  if (['preview', 'realtime-preview'].includes(canvas?.id)) return null;
  return canvas?.dataset?.skinCanvas ?? Object.entries(EDITOR_SKIN_CANVASES).find(([, selector]) => selector && (selector === '#' + canvas?.id || canvas?.matches?.(selector)))?.[0];
}
export function editorCanvasOpacity(canvas, requestedRole) {
  const role = requestedRole ?? canvasRole(canvas);
  if (!settings || !Object.hasOwn(EDITOR_SKIN_CANVASES, role) || ['preview', 'realtime-preview'].includes(canvas?.id)) return 1;
  const config = { ...settings, ...settings.targets?.[role] }; return config.opacity === undefined ? 1 : config.opacity;
}
export function editorSkinContext(context, requestedRole) {
  const role = requestedRole ?? canvasRole(context?.canvas);
  if (!context || !settings || !Object.hasOwn(EDITOR_SKIN_CANVASES, role) || ['preview', 'realtime-preview'].includes(context.canvas?.id)) return context;
  let roles = contexts.get(context); if (!roles) { roles = new Map(); contexts.set(context, roles); }
  if (roles.has(role)) return roles.get(role);
  const bound = new Map(); const colors = new Map(); let generation = -1; let config;
  const update = () => {
    if (generation !== revision) { colors.clear(); generation = revision; const local = settings?.targets?.[role]; config = settings ? { ...settings, ...local, palette: { ...settings.palette, ...local?.palette } } : null; }
  };
  const color = value => { update(); if (!config || typeof value !== 'string') return value; if (!colors.has(value)) colors.set(value, recolor(value, config.palette)); return colors.get(value); };
  const proxy = new Proxy(context, {
    get(target, key) {
      update();
      if (['createLinearGradient', 'createRadialGradient', 'createConicGradient'].includes(key)) return (...args) => {
        const gradient = target[key](...args); const add = gradient.addColorStop.bind(gradient); gradient.addColorStop = (offset, value) => add(offset, color(value)); return gradient;
      };
      if (key === 'setLineDash') return values => target.setLineDash(values.length && config?.lineDash ? config.lineDash : values);
      const value = Reflect.get(target, key, target);
      if (typeof value !== 'function') return value;
      if (!bound.has(key)) bound.set(key, ['fill', 'stroke', 'fillRect', 'strokeRect', 'fillText', 'strokeText', 'drawImage'].includes(key) ? (...args) => {
        update(); const alpha = target.globalAlpha; const width = target.lineWidth;
        if (config?.opacity !== undefined) target.globalAlpha = alpha * config.opacity;
        if (config?.lineWidthScale !== undefined) target.lineWidth = width * config.lineWidthScale;
        try { return value.apply(target, args); } finally { target.globalAlpha = alpha; target.lineWidth = width; }
      } : value.bind(target)); return bound.get(key);
    },
    set(target, key, value) {
      update();
      if (typeof value === 'string' && ['fillStyle', 'strokeStyle', 'shadowColor'].includes(key)) value = color(value);
      else if (config && key === 'font' && typeof value === 'string') {
        if (config.fontScale) value = value.replace(/([\d.]+)px/, (match, size) => `${Number(size) * config.fontScale}px`);
        if (config.resolvedFont) value = value.replace(/RPE(?:Game)?(?=[, ]|$)/g, `"${config.resolvedFont}"`);
      }
      return Reflect.set(target, key, value, target);
    },
  });
  roles.set(role, proxy); originalContexts.set(proxy, context); return proxy;
}

function recolor(value, palette) {
  let rgb; let alpha = 1;
  if (/^#[\da-f]{3,8}$/i.test(value)) {
    let hex = value.slice(1); if ([3, 4].includes(hex.length)) hex = [...hex].map(char => char + char).join('');
    if (![6, 8].includes(hex.length)) return value;
    rgb = hex.slice(0, 6).toLowerCase(); if (hex.length === 8) alpha = parseInt(hex.slice(6), 16) / 255;
  } else {
    const match = value.match(/^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)(?:\s*,\s*([\d.]+))?\s*\)$/i);
    if (!match) return value;
    rgb = match.slice(1, 4).map(number => Math.round(Number(number)).toString(16).padStart(2, '0')).join(''); if (match[4] !== undefined) alpha = Number(match[4]);
  }
  const next = palette['#' + rgb]; if (!next) return value;
  if (alpha === 1) return next;
  const [red, green, blue] = [1, 3, 5].map(offset => parseInt(next.slice(offset, offset + 2), 16));
  return `rgba(${red},${green},${blue},${alpha})`;
}
