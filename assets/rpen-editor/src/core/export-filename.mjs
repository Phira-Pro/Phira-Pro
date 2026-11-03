const encoder = new TextEncoder();
const extensions = /(?:\.(?:json|pez|pec|zip))+$/i;
const key = name => name.normalize('NFC').toLowerCase();

function shorten(text, limit = 180) {
  let result = '', length = 0;
  for (const character of text) {
    const size = encoder.encode(character).length;
    if (length + size > limit) break;
    result += character; length += size;
  }
  return result.replace(/[. ]+$/, '');
}

export function safeExportStem(value, { stripExtension = true } = {}) {
  let stem = String(value ?? '').normalize('NFC').trim();
  if (stripExtension) stem = stem.replace(extensions, '');
  stem = stem.replace(/[<>:"/\\|?*\x00-\x1f\x7f]/g, '_').replace(/[. ]+$/, '').replace(/^\.+/, '');
  stem = shorten(stem) || 'Untitled';
  if (/^(?:con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(?:\.|$)/i.test(stem)) stem = '_' + stem;
  return stem;
}

export function chartExportStem(chart, chartName = '') {
  const title = typeof chart?.META?.name === 'string' ? chart.META.name.trim() : '';
  return safeExportStem(title || String(chartName).replaceAll('\\', '/').split('/').at(-1));
}

export function safeDownloadName(value) {
  const name = String(value ?? '').trim(), match = name.match(/^(.*)(\.[a-z0-9]{1,16})$/i);
  return match ? safeExportStem(match[1], { stripExtension: false }) + match[2] : safeExportStem(name, { stripExtension: false });
}

// Download destinations are private to the browser. Avoid repeats within this
// session; the browser also resolves names already present on disk.
export class DownloadNames {
  constructor() { this.used = new Set(); }
  suggest(value) {
    const name = safeDownloadName(value), dot = name.lastIndexOf('.');
    const stem = dot > 0 ? name.slice(0, dot) : name, extension = dot > 0 ? name.slice(dot) : '';
    let candidate = name, index = 2;
    while (this.used.has(key(candidate))) candidate = stem + ' (' + index++ + ')' + extension;
    return candidate;
  }
  reserve(value) { const name = this.suggest(value); this.used.add(key(name)); return name; }
}
