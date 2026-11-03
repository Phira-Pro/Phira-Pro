import { IntervalIndex } from './interval-index.mjs';
import { upperBound } from './beat.mjs';
import { EVENT_TYPES } from './chart.mjs';

export function stepLine(index, direction, count) {
  return count > 0 ? ((index + Math.sign(direction)) % count + count) % count : null;
}

export function stepOverviewLine(index, direction, indices) {
  if (!indices.length || !direction) return null;
  const position = indices.indexOf(index);
  if (position < 0) return direction > 0 ? indices[0] : indices.at(-1);
  return indices[stepLine(position, direction, indices.length)];
}

export function lineOverviewWindow(index, count, columns, rows, browseRow = null) {
  const totalRows = Math.ceil(count / columns);
  const maxRow = Math.max(0, totalRows - rows);
  const firstRow = Math.max(0, Math.min(maxRow, browseRow ?? Math.floor(index / columns) - Math.floor(rows / 2)));
  const first = firstRow * columns;
  return { totalRows, maxRow, firstRow, indices: Array.from({ length: Math.max(0, Math.min(columns * rows, count - first)) }, (unused, offset) => first + offset) };
}

export function nearbyLines(index, count, columns = 5, rows = 4) {
  return lineOverviewWindow(index, count, columns, rows).indices;
}

export function lineOverviewLayout(width, height) {
  const panelWidth = Math.min(width - 16, width * 0.88, 1060);
  const columns = Math.max(2, Math.min(6, Math.floor((panelWidth - 36) / 140)));
  const previousRows = height < 300 ? 2 : 3;
  const thumbnailHeight = Math.floor(Math.max(24, Math.min(96, (height * 0.84 - 36) / previousRows - 26)));
  return { columns, rows: previousRows + 1, thumbnailHeight, panelWidth };
}

export class LineOverviewIndex {
  constructor(line, tempo, shaders = [], layer = undefined, extended = false) {
    const factor = line.bpmfactor ?? 1;
    const timing = item => ({ item, start: tempo.seconds(item.startTime, factor), end: tempo.seconds(item.endTime, factor) });
    const notes = (line.notes ?? []).map(timing);
    const events = [];
    if (extended) {
      const extendedTypes = ['scaleXEvents', 'scaleYEvents', 'colorEvents', 'paintEvents', 'textEvents'];
      for (const [type, list] of Object.entries(line.extended ?? {})) {
        const channel = extendedTypes.indexOf(type);
        if (channel < 0 || !Array.isArray(list)) continue;
        for (const event of list) if (event?.startTime && event?.endTime) events.push({ ...timing(event), channel, layer: 0 });
      }
      for (const event of shaders) events.push({ ...timing(event), channel: 3, layer: 0 });
    } else {
      const layers = layer === undefined ? (line.eventLayers ?? []) : [line.eventLayers?.[layer] ?? {}];
      layers.forEach((currentLayer, layerIndex) => EVENT_TYPES.forEach((type, channel) => {
        for (const event of currentLayer[type] ?? []) events.push({ ...timing(event), channel, layer: layer === undefined ? layerIndex : layer });
      }));
      if (layer === undefined) for (const [type, list] of Object.entries(line.extended ?? {})) {
        if (type === 'paintEvents' || !Array.isArray(list)) continue;
        for (const event of list) if (event?.startTime && event?.endTime) events.push({ ...timing(event), channel: 5, layer: 0 });
      }
      // Preserve the direct-constructor API used by diagnostics and plugins.
      // The line switcher passes an empty shader list for ordinary layers.
      for (const event of shaders) events.push({ ...timing(event), channel: 5, layer });
    }
    this.notes = new IntervalIndex(notes, entry => entry.start, entry => entry.end);
    this.events = new IntervalIndex(events, entry => entry.start, entry => entry.end);
    this.noteEnds = notes.map(entry => entry.end).sort((left, right) => left - right);
    this.eventEnds = events.map(entry => entry.end).sort((left, right) => left - right);
  }

  matches(start, end, notesOnly, eventsOnly) {
    return (!notesOnly || this.notes.has(start, end)) && (!eventsOnly || this.events.has(start, end));
  }

  sample(seconds, start, end) {
    return {
      notes: this.notes.query(start, end).map(entry => entry.item),
      events: this.events.query(start, end).map(entry => entry.item),
      notesLeft: this.noteEnds.length - upperBound(this.noteEnds, seconds, value => value),
      eventsLeft: this.eventEnds.length - upperBound(this.eventEnds, seconds, value => value),
    };
  }
}
