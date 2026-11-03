function integerFields(item, fields) {
  let result = item;
  for (const field of fields) {
    const value = item[field];
    if (Number.isFinite(value) && !Number.isInteger(value)) result = { ...result, [field]: Math.trunc(value) };
  }
  return result;
}

export const integerAlphaEvent = event => integerFields(event, ['start', 'end']);
export const integerNoteAlpha = note => integerFields(note, ['alpha']);

function mapChanged(items, transform) {
  let result = items;
  for (let index = 0; index < (items?.length ?? 0); index++) {
    const next = transform(items[index], index);
    if (next === items[index]) continue;
    if (result === items) result = [...items];
    result[index] = next;
  }
  return result;
}

export function integerChartAlpha(chart, previous) {
  const lines = mapChanged(chart.judgeLineList, (line, index) => {
    const oldLine = previous?.judgeLineList?.[index];
    if (line === oldLine) return line;
    const notes = line.notes === oldLine?.notes ? line.notes : mapChanged(line.notes, integerNoteAlpha);
    const layers = mapChanged(line.eventLayers, (layer, layerIndex) => {
      if (!layer || layer === oldLine?.eventLayers?.[layerIndex]) return layer;
      const events = layer.alphaEvents === oldLine?.eventLayers?.[layerIndex]?.alphaEvents ? layer.alphaEvents : mapChanged(layer.alphaEvents, integerAlphaEvent);
      return events === layer.alphaEvents ? layer : { ...layer, alphaEvents: events };
    });
    return notes === line.notes && layers === line.eventLayers ? line : { ...line, notes, eventLayers: layers };
  });
  return lines === chart.judgeLineList ? chart : { ...chart, judgeLineList: lines };
}
