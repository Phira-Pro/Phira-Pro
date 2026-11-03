export const validSelectionInterval = value => Number.isSafeInteger(value) && value >= 1;

export function intervalSelection(entries, interval = 1) {
  const candidates = entries.filter(entry => !entry.selected);
  if (!validSelectionInterval(interval) || interval === 1) return candidates;
  const groups = new Map();
  for (const entry of candidates) {
    const group = entry.group ?? '';
    if (!groups.has(group)) groups.set(group, []);
    groups.get(group).push(entry);
  }
  return [...groups.values()].flatMap(group => group.sort((left, right) => left.start - right.start).filter((entry, index) => index % interval === 0));
}

export function extendIntervalSelection(drag, entries, selection, interval = 1) {
  if (!validSelectionInterval(interval) || interval === 1) {
    for (const entry of entries) drag.remove ? selection.delete(entry.key) : selection.add(entry.key);
    return;
  }
  const state = drag.intervalSelection ??= { base: new Set(selection), candidates: new Map(), chosen: [] };
  let changed = false;
  for (const entry of entries) {
    if (state.base.has(entry.key) !== Boolean(drag.remove) || state.candidates.has(entry.key)) continue;
    state.candidates.set(entry.key, entry); changed = true;
  }
  if (!changed) return;
  for (const entry of state.chosen) drag.remove ? selection.add(entry.key) : selection.delete(entry.key);
  state.chosen = intervalSelection([...state.candidates.values()], interval);
  for (const entry of state.chosen) drag.remove ? selection.delete(entry.key) : selection.add(entry.key);
}
