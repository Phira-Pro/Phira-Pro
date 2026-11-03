import { assertChart, createEvent, createNote, EVENT_TYPES } from './chart.mjs';
import { beatValue, fromNumber, parseBeat } from './beat.mjs';
import { integerChartAlpha } from './alpha-values.mjs';
import { easing } from './easing.mjs';
import { createNoiseArea, createNoiseEvent } from './noise-areas.mjs';
import { IntervalIndex } from './interval-index.mjs';
import { t } from '../i18n.mjs';

export const agentBeat = value => Array.isArray(value) ? beatValue(value) : typeof value === 'number' ? beatValue(fromNumber(value)) : beatValue(parseBeat(value));
const own = (target, key, value) => Object.defineProperty(target, key, { value, enumerable: true, writable: true, configurable: true });
export function agentPath(path = '') {
  if (path === '') return [];
  if (typeof path !== 'string' || !path.startsWith('/') || /~(?![01])/u.test(path)) throw Error(t('agent-chart.s73d9b70'));
  return path.slice(1).split('/').map(key => key.replaceAll('~1', '/').replaceAll('~0', '~'));
}
export function agentAt(root, path) {
  let value = root;
  for (const key of agentPath(path)) {
    if (!value || typeof value !== 'object' || !Object.hasOwn(value, key)) throw Error(t('agent-chart.sbe3a3bb') + path);
    value = value[key];
  }
  return value;
}
export function agentChartPatch(chart, operations) {
  let next = structuredClone(chart); let dangerous = false;
  for (const operation of operations) {
    const path = agentPath(operation.path);
    if (!path.length) { if (operation.op !== 'set') throw Error(t('agent-chart.s5c20e3a')); next = structuredClone(operation.value); dangerous = true; continue; }
    const root = path[0];
    if (root === 'judgeLineList' && (path.length <= 2 || path[2] === 'notes' && (operation.op === 'remove' || operation.op === 'set') && path.length === 3)) dangerous = true;
    if (root === 'META' && ['song', 'background'].includes(path[1]) || root === 'globalEvents' && operation.op === 'remove') dangerous = true;
    let parent = next;
    for (const key of path.slice(0, -1)) {
      if (!parent || typeof parent !== 'object' || !Object.hasOwn(parent, key)) throw Error(t('agent-chart.sbfb50d8') + operation.path);
      parent = parent[key];
    }
    if (!parent || typeof parent !== 'object') throw Error(t('agent-chart.sac28526'));
    const key = path.at(-1);
    if (Array.isArray(parent) && !/^(0|[1-9]\d*|-)$/.test(key)) throw Error(t('agent-chart.sccaeaba'));
    if (operation.op === 'remove') {
      if (!Object.hasOwn(parent, key)) throw Error(t('agent-chart.s1630e34'));
      if (Array.isArray(parent)) parent.splice(Number(key), 1); else delete parent[key];
    } else if (operation.op === 'insert') {
      if (!Array.isArray(parent)) throw Error(t('agent-chart.s6aef484'));
      const index = key === '-' ? parent.length : Number(key);
      if (index > parent.length) throw Error(t('agent-chart.s51c39d6'));
      parent.splice(index, 0, structuredClone(operation.value));
    } else if (operation.op === 'append') {
      if (!Object.hasOwn(parent, key) || !Array.isArray(parent[key]) || !Array.isArray(operation.value)) throw Error(t('agent-chart.sbdbe365'));
      if (parent[key].length + operation.value.length > 1000000) throw Error(t('agent-chart.s866c771'));
      for (const entry of operation.value) parent[key].push(structuredClone(entry));
    } else if (operation.op === 'set') {
      if (Array.isArray(parent) && (key === '-' || Number(key) >= parent.length)) throw Error(t('agent-chart.sb100225'));
      own(parent, key, structuredClone(operation.value));
    } else throw Error(t('agent-chart.sec690a3'));
  }
  next = integerChartAlpha(next, chart);
  for (const line of next.judgeLineList ?? []) line.numOfNotes = line.notes?.length ?? 0;
  const notesBefore = (chart.judgeLineList ?? []).reduce((total, line) => total + (line.notes?.length ?? 0), 0);
  const notesAfter = (next.judgeLineList ?? []).reduce((total, line) => total + (line.notes?.length ?? 0), 0);
  if (notesBefore - notesAfter > 50 || (chart.blockAreaList?.length ?? 0) - (next.blockAreaList?.length ?? 0) > 50) dangerous = true;
  assertChart(next); return { chart: next, dangerous };
}
export function agentChartSummary(chart) {
  return { metadata: structuredClone(chart.META), bpm: structuredClone(chart.BPMList), lines: chart.judgeLineList?.length ?? 0,
    notes: (chart.judgeLineList ?? []).reduce((total, line) => total + (line.notes?.length ?? 0), 0), noiseAreas: chart.blockAreaList?.length ?? 0,
    noiseLayers: structuredClone(chart.noiseAreaLayers ?? []), globalEventTypes: Object.keys(chart.globalEvents?.types ?? {}) };
}
function eventMerge(events, added, replace) {
  const first = beatValue(added[0].startTime), last = beatValue(added.at(-1).endTime);
  const overlap = event => beatValue(event.startTime) < last && beatValue(event.endTime) > first;
  if (!replace && events.some(overlap)) throw Error(t('agent-chart.s0db4a0d'));
  const remaining = [];
  for (const event of events) {
    if (!overlap(event)) { remaining.push(event); continue; }
    if (beatValue(event.startTime) < first || beatValue(event.endTime) > last) throw Error(t('agent-chart.sadbca15'));
  }
  return [...remaining, ...added].sort((left, right) => beatValue(left.startTime) - beatValue(right.startTime));
}
export function agentShake(chart, options) {
  const { line: index, track, layer = 0, amplitude, cycles, segmentsPerCycle = 16, decayEasing = 1, baseline = 0, replace = false } = options;
  const first = agentBeat(options.start), duration = agentBeat(options.duration);
  if (!['moveXEvents', 'moveYEvents', 'rotateEvents'].includes(track) || !(duration > 0) || !(cycles > 0) || !Number.isInteger(decayEasing) || decayEasing < 1 || decayEasing > 29) throw Error(t('agent-chart.s23b4a0f'));
  const count = Math.ceil(cycles * segmentsPerCycle); if (count > 2000) throw Error(t('agent-chart.s17948c3'));
  const next = structuredClone(chart), line = next.judgeLineList[index]; if (!line) throw Error(t('agent-chart.s181a573'));
  while (line.eventLayers.length <= layer) line.eventLayers.push(Object.fromEntries(EVENT_TYPES.map(type => [type, []])));
  const value = progress => baseline + amplitude * (1 - Math.max(0, Math.min(1, easing(progress, decayEasing)))) * Math.sin(progress * cycles * Math.PI * 2);
  const events = Array.from({ length: count }, (unused, offset) => createEvent(value(offset / count), offset === count - 1 ? baseline : value((offset + 1) / count), first + duration * offset / count, first + duration * (offset + 1) / count));
  line.eventLayers[layer][track] = eventMerge(line.eventLayers[layer][track] ?? [], events, replace);
  assertChart(next); return { chart: next, count, first, end: first + duration };
}
export function agentCircle(chart, options) {
  const first = agentBeat(options.start), duration = agentBeat(options.duration);
  const { centerX, centerY, radius, width, height, turns = 1, phase = 0, clockwise = true, samples = 128, active = true, invert = false, layerId = 'default', name = t('agent-chart.s3b41944') } = options;
  if (!(duration > 0) || !(radius > 0) || !(width > 0) || !(height > 0) || !(turns > 0)) throw Error(t('agent-chart.sa9e9341'));
  const area = createNoiseArea(first, first + duration); area.Name = name; area.layerId = layerId; area.isInvert = invert;
  area.bottomLeft = { x: -width / 2, y: -height / 2 }; area.topRight = { x: width / 2, y: height / 2 };
  const point = progress => { const angle = (phase + (clockwise ? -1 : 1) * turns * 360 * progress) * Math.PI / 180; return { x: centerX + radius * Math.cos(angle), y: centerY + radius * Math.sin(angle) }; };
  const count = Math.max(8, Math.ceil(samples * turns)); if (count > 2000) throw Error(t('agent-chart.sb4b0dbe'));
  for (let index = 0; index < count; index++) {
    const begin = point(index / count), end = point((index + 1) / count);
    for (const [axis, type] of [['x', 'moveXEvents'], ['y', 'moveYEvents']]) area[type].push(createNoiseEvent(begin[axis], end[axis], first + duration * index / count, first + duration * (index + 1) / count));
  }
  if (active) area.activeIntervals = [{ startTime: area.appearTime, endTime: area.disappearTime, readyFlash: 0.5 }];
  const next = { ...chart, blockAreaList: [...(chart.blockAreaList ?? []), area] }; assertChart(next); return { chart: next, index: next.blockAreaList.length - 1, count: count * 2 };
}
export function agentTapNotes(chart, lineIndex, beats, positionX = 0) {
  const next = structuredClone(chart), line = next.judgeLineList[lineIndex]; if (!line) throw Error(t('agent-chart.s181a573'));
  const existing = new Set((line.notes ?? []).filter(note => !note.isFake && [1, 2].includes(note.type) && note.positionX === positionX).map(note => beatValue(note.startTime).toFixed(6)));
  const holds = new IntervalIndex((line.notes ?? []).filter(note => !note.isFake && note.type === 2 && Math.abs(note.positionX - positionX) < 1e-7), note => beatValue(note.startTime), note => beatValue(note.endTime));
  const added = [];
  for (const beat of beats) { const time = agentBeat(beat); if (!existing.has(time.toFixed(6)) && !holds.has(time - 1e-7, time + 1e-7)) { added.push(createNote(1, time, positionX)); existing.add(time.toFixed(6)); } }
  line.notes = [...(line.notes ?? []), ...added].sort((left, right) => beatValue(left.startTime) - beatValue(right.startTime)); line.numOfNotes = line.notes.length;
  assertChart(next); return { chart: next, count: added.length };
}
