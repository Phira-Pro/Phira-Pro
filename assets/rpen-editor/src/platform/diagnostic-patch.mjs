const special = new Set(['judgeLineList', 'blockAreaList']);
function recordPatch(previous = {}, next = {}, omitted = new Set()) {
  previous ??= {}; next ??= {};
  const set = {}, remove = [];
  for (const key of Object.keys(next)) if (!omitted.has(key) && previous[key] !== next[key]) Object.defineProperty(set, key, { value: next[key], enumerable: true });
  for (const key of Object.keys(previous)) if (!omitted.has(key) && !Object.hasOwn(next, key)) remove.push(key);
  return { set, remove };
}
function arrayPatch(previous = [], next = [], transform = (before, value) => ({ replace: value })) {
  previous ??= []; next ??= [];
  return { length: next.length, updates: next.flatMap((value, index) => previous[index] === value ? [] : [{ index, ...transform(previous[index], value) }]) };
}
function linePatch(previous, next) {
  if (!previous) return { replace: next };
  const patch = recordPatch(previous, next, new Set(['eventLayers', 'extended']));
  if (previous.eventLayers !== next.eventLayers) patch.layers = arrayPatch(previous.eventLayers, next.eventLayers, (a, b) => b ? recordPatch(a, b) : { replace: b });
  if (previous.extended !== next.extended) patch.extended = next.extended ? recordPatch(previous.extended, next.extended) : { replace: next.extended };
  return patch;
}
export function diagnosticPatch(previous, next) {
  if (!previous) return { replace: next };
  const patch = recordPatch(previous, next, special);
  if (previous.judgeLineList !== next.judgeLineList) patch.lines = arrayPatch(previous.judgeLineList, next.judgeLineList, linePatch);
  if (previous.blockAreaList !== next.blockAreaList) patch.areas = arrayPatch(previous.blockAreaList, next.blockAreaList);
  return patch;
}
function applyRecord(previous, patch) { if (Object.hasOwn(patch, 'replace')) return patch.replace; const result = { ...previous, ...patch.set }; for (const key of patch.remove ?? []) delete result[key]; return result; }
function applyArray(previous = [], patch, transform = applyRecord) { const values = previous.slice(0, patch.length); values.length = patch.length; for (const entry of patch.updates) values[entry.index] = transform(previous[entry.index], entry); return values; }
export function applyDiagnosticPatch(previous, patch) {
  const next = applyRecord(previous, patch);
  if (patch.lines) next.judgeLineList = applyArray(previous.judgeLineList, patch.lines, (line, entry) => {
    const value = applyRecord(line, entry); if (entry.layers) value.eventLayers = applyArray(line.eventLayers, entry.layers); if (entry.extended) value.extended = applyRecord(line.extended, entry.extended); return value;
  });
  if (patch.areas) next.blockAreaList = applyArray(previous.blockAreaList, patch.areas);
  return next;
}
