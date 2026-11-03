const noteCount = chart => (chart.judgeLineList ?? []).reduce((total, line) => total + (line.notes?.length ?? 0), 0);
const eventCount = chart => (chart.judgeLineList ?? []).reduce((total, line) => total + [...(line.eventLayers ?? []).flatMap(layer => Object.values(layer ?? {})), ...Object.values(line.extended ?? {})].reduce((count, events) => count + (Array.isArray(events) ? events.length : 0), 0), 0);

export function agentScriptChanges(before, after, origins) {
  const notesBefore = noteCount(before), notesAfter = noteCount(after), eventsBefore = eventCount(before), eventsAfter = eventCount(after);
  const layers = new Set(origins.filter(index => index !== null));
  const removedLines = before.judgeLineList.length - layers.size;
  const reordered = origins.some((index, position) => index !== null && index !== position);
  const removedRootFields = Object.keys(before).filter(key => !Object.hasOwn(after, key));
  const noiseBefore = before.blockAreaList?.length ?? 0, noiseAfter = after.blockAreaList?.length ?? 0;
  return { notesBefore, notesAfter, eventsBefore, eventsAfter, noiseBefore, noiseAfter, removedLines, reordered, removedRootFields,
    dangerous: removedLines > 0 || reordered || removedRootFields.length > 0 || notesBefore - notesAfter > 50 || eventsBefore - eventsAfter > 50 || noiseBefore - noiseAfter > 50 || before.META.song !== after.META.song || before.META.background !== after.META.background };
}
