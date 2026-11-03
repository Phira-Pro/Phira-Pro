import { createLine, createNote, noteIsAbove } from './chart.mjs';
import { DEFAULT_NOTE_TINT, DEFAULT_HIT_TINT } from './note-properties.mjs';

export const DEFAULT_NOTE_PLACEMENT = Object.freeze({ above: 1, isFake: 0, speed: 1, size: 1, judgeArea: 1,
  alpha: 255, yOffset: 0, visibleTime: 999999, tint: DEFAULT_NOTE_TINT, tintHitEffects: DEFAULT_HIT_TINT, hitsound: '' });
export const DEFAULT_LINE_EVENTS = Object.freeze({ moveXEvents: 0, moveYEvents: 0, rotateEvents: 0, alphaEvents: 255, speedEvents: 10 });

const record = value => value && typeof value === 'object' && !Array.isArray(value) ? value : {};
const color = value => Array.isArray(value) && value.length === 3 && value.every(channel => Number.isInteger(channel) && channel >= 0 && channel <= 255);
export function normalizeNoteDefaults(value) {
  value = record(value); const result = { ...DEFAULT_NOTE_PLACEMENT, tint: [...DEFAULT_NOTE_TINT], tintHitEffects: [...DEFAULT_HIT_TINT] };
  for (const key of ['speed', 'size', 'judgeArea', 'alpha', 'yOffset', 'visibleTime']) {
    if (!Object.hasOwn(value, key) || !Number.isFinite(value[key])) continue;
    if (key === 'size' && value[key] <= 0 || key === 'visibleTime' && value[key] < 0) continue;
    result[key] = key === 'alpha' ? Math.max(0, Math.min(255, Math.trunc(value[key]))) : value[key];
  }
  if (Object.hasOwn(value, 'above') && [0, 1, 2].includes(Number(value.above))) result.above = noteIsAbove(value) ? 1 : 2;
  if (Object.hasOwn(value, 'isFake')) result.isFake = Number(value.isFake) === 1 ? 1 : 0;
  for (const key of ['tint', 'tintHitEffects']) if (Object.hasOwn(value, key) && color(value[key])) result[key] = [...value[key]];
  if (Object.hasOwn(value, 'hitsound') && typeof value.hitsound === 'string') result.hitsound = value.hitsound.trim();
  return result;
}
export function normalizeLineDefaults(value) {
  value = record(value); const result = { ...DEFAULT_LINE_EVENTS };
  for (const key of Object.keys(result)) if (Object.hasOwn(value, key) && Number.isFinite(value[key])) result[key] = key === 'alphaEvents' ? Math.min(255, Math.trunc(value[key])) : value[key];
  return result;
}

export function applyNoteDefaults(note, defaults) {
  const values = normalizeNoteDefaults(defaults), result = { ...note };
  for (const key of Object.keys(DEFAULT_NOTE_PLACEMENT)) {
    if (key === 'above') result.above = values.above === 1 ? 1 : note.type === 2 ? 0 : 2;
    else if (Object.hasOwn(note, key) || (Array.isArray(values[key]) ? values[key].some((channel, index) => channel !== DEFAULT_NOTE_PLACEMENT[key][index]) : values[key] !== DEFAULT_NOTE_PLACEMENT[key])) result[key] = Array.isArray(values[key]) ? [...values[key]] : values[key];
  }
  return result;
}
export function createPlacedNote(type, beat, positionX, endBeat, defaults) { return applyNoteDefaults(createNote(type, beat, positionX, endBeat), defaults); }
export function createPlacedLine(name, defaults) {
  const line = createLine(name), values = normalizeLineDefaults(defaults);
  for (const [type, value] of Object.entries(values)) line.eventLayers[0][type][0].start = line.eventLayers[0][type][0].end = value;
  return line;
}
