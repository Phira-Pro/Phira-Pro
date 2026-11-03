import { assetPath, resourceReferences } from './files.mjs';
import { parentFolder } from '../core/asset-library.mjs';

// Prefer chart-relative references only when they resolve to the intended file.
export function assetReference(assets, path, chartName = '') {
  const directory = parentFolder(chartName), relative = directory && path.startsWith(directory + '/') ? path.slice(directory.length + 1) : path;
  if (assetPath(assets, relative, chartName) === path) return relative;
  if (assetPath(assets, path, chartName) === path) return path;
  const a = directory.split('/').filter(Boolean), b = path.split('/');
  while (a.length && b.length && a[0] === b[0]) { a.shift(); b.shift(); }
  return '../'.repeat(a.length) + b.join('/');
}

export function remapAssetReferences(chart, before, after, paths, chartName = '') {
  if (!paths?.size) return chart;
  const reference = name => { const original = assetPath(before, name, chartName), target = paths.get(original); return target && target !== original ? assetReference(after, target, chartName) : name; };
  let changed = false;
  const lines = chart.judgeLineList.map(line => {
    const texture = reference(line.Texture);
    let notesChanged = false;
    const notes = line.notes?.map(note => {
      let next = note;
      for (const key of ['hitsound', 'hitSound']) if (typeof note[key] === 'string') {
        const sound = reference(note[key]); if (sound !== note[key]) { next = { ...next, [key]: sound }; notesChanged = true; }
      }
      return next;
    });
    if (texture === line.Texture && !notesChanged) return line;
    changed = true; return { ...line, Texture: texture, ...(notesChanged ? { notes } : {}) };
  });
  const sources = resourceReferences(chart, before, chartName), meta = { ...chart.META };
  for (const key of ['background', 'song']) {
    const name = chart.META[key] || sources[key], target = reference(name);
    if (target !== name) { meta[key] = target; changed = true; }
  }
  return changed ? { ...chart, META: meta, judgeLineList: lines } : chart;
}
