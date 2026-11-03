import { agentSchema, agentNumber, agentText, agentIndex } from '../core/agent-commands.mjs';
import { agentAt, agentPath, agentBeat, agentChartPatch, agentChartSummary, agentCircle, agentShake, agentTapNotes } from '../core/agent-chart.mjs';
import { analyzeAgentAudio, quantizeAgentPeaks } from '../core/agent-audio.mjs';
import { mapperTiming } from '../core/audio-mapper.mjs';
import { assertChart, diagnose, createNote, createEvent, createLine } from '../core/chart.mjs';
import { createNoiseArea, createNoiseEvent } from '../core/noise-areas.mjs';
import { beatValue, fromNumber } from '../core/beat.mjs';
import { deleteLine, duplicateLine, ensureLineCount, reorderLine, setLineParent } from './line-commands.mjs';
import { TempoMap } from '../core/tempo.mjs';
import { compileExpression } from '../core/batch-script.mjs';
import { AgentSourceReader } from '../platform/agent-source.mjs';
import { registerAgentChartTools } from './agent-chart-tools.mjs';
import { registerAgentWorkspaceTools } from './agent-workspace-tools.mjs';
import { registerAgentScriptingTools } from './agent-scripting-tools.mjs';
import { registerAgentFileTools } from './agent-file-tools.mjs';
import { registerAgentComponentTools } from './agent-component-tools.mjs';
import { t } from '../i18n.mjs';

const beatSchema = { description: t('agent-editor.s68d1f7f') };
const number = (minimum, maximum) => ({ type: 'number', minimum, maximum });
const integer = (minimum, maximum) => ({ type: 'integer', minimum, maximum });
const enumeration = values => ({ type: 'string', enum: values });
const array = (items, maxItems = 2000) => ({ type: 'array', items, maxItems });
const pathOf = path => { if (typeof path !== 'string' || !path || path.length > 512 || path.includes('\\') || path.startsWith('/') || path.split('/').some(part => !part || ['.', '..'].includes(part))) throw Error(t('agent-editor.s2b5fd94')); return path; };
const contentBytes = bytes => bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);

export function createAgentEditorCommands(commands, editor, { controls, network, ask, capture, captureEditor, prependSilence, patterns = {}, source = new AgentSourceReader(), workspace } = {}) {
  const revisions = new WeakMap(); let serial = 0;
  const revision = chart => { if (!revisions.has(chart)) revisions.set(chart, 'chart_' + ++serial); return revisions.get(chart); };
  const current = () => { if (!editor.hasDocument) throw Error(t('agent-editor.s21c0425')); return editor.chart; };
  const check = args => { const chart = current(); if (args.revision !== revision(chart)) throw Error(t('agent-editor.sc9f3b20')); return chart; };
  const prepareChart = (args, chart, next, summary, result = {}, dangerous = false) => {
    assertChart(next); const session = editor.session;
    return { summary, access: dangerous ? 'dangerous' : 'edit', details: { before: agentChartSummary(chart), after: agentChartSummary(next), ...result }, valid: () => editor.session === session && editor.chart === chart,
      apply: () => { editor.commit('AI · ' + summary, next); if (editor.chart === chart) throw Error(t('agent-editor.s02c0013')); return { ...result, revision: revision(editor.chart), undoable: true }; } };
  };
  const register = (name, description, parameters, access, prepare) => commands.register({ name, description, parameters, access, prepare });
  register('editor_source', t('agent-editor.s8fa3b92'), agentSchema({ action: enumeration(['list', 'search', 'read']), path: agentText, query: { type: 'string', maxLength: 200 }, offset: agentIndex, limit: integer(1, 100), startLine: integer(1, Number.MAX_SAFE_INTEGER), startColumn: integer(1, Number.MAX_SAFE_INTEGER), lineCount: integer(1, 200), caseSensitive: { type: 'boolean' } }), 'read', (args, { signal }) => ({ apply: () => source.query(args, signal) }));
  register('chart_templates', t('agent-editor.s3edd547'), agentSchema({ command: agentText }), 'read', args => ({ apply: () => args.command ? commands.tools().find(tool => tool.function.name === args.command)?.function ?? { error: t('agent-editor.s4b3be4d') } : {
    note: createNote(1, 0, 0), hold: createNote(2, 0, 0, 1), event: createEvent(0, 0, 0, 1), line: createLine(), noiseArea: createNoiseArea(0, 4), noiseTransform: createNoiseEvent(0, 0, 0, 1, { x: 0, y: 0 }),
    scriptApi: { createNote: t('agent-editor.s63f9b5f'), createEvent: '(start,end,startBeat,endBeat)', createLine: '(name?)', createNoiseArea: '(startBeat,endBeat)', createNoiseEvent: '(start,end,startBeat,endBeat,anchor?)', fromNumber: t('agent-editor.s27fbb3b'), beatValue: t('agent-editor.s357cb4b'), parseBeat: t('agent-editor.sc43a600'), easing: '(progress,id)' },
    guide: t('agent-editor.s8927de4') } }));
  register('editor_context', t('agent-editor.s597c6c6'), agentSchema(), 'read', () => ({ apply: () => {
    const chart = current(); return { ...agentChartSummary(chart), revision: revision(chart), currentLine: editor.session.lineIndex, layer: editor.timeline.layer, beat: editor.currentBeat(), chartSeconds: editor.chartSeconds(), audioSeconds: editor.audio.time,
      audio: { name: chart.META.song, duration: editor.audio.duration, decoded: Boolean(editor.audio.buffer), sampleRate: editor.audio.buffer?.sampleRate },
      selection: { notes: [...editor.session.selection], events: [...editor.session.eventSelection], noise: [...editor.noiseEditor.selection], noiseOwner: editor.noiseEditor.selected },
      pane: editor.activePane, playing: editor.audio.playing, timeBases: { chartSeconds: editor.chartSeconds(), musicSeconds: editor.audio.time, offsetMilliseconds: chart.META.offset ?? 0, globalBeat: new TempoMap(chart.BPMList).beat(editor.chartSeconds()), lineBeat: new TempoMap(chart.BPMList).beat(editor.chartSeconds(), chart.judgeLineList[editor.session.lineIndex]?.bpmfactor ?? 1) },
      markers: (chart.markers ?? []).slice(0, 100).map((marker, index) => ({ index, name: marker.name, chartSeconds: marker.time, musicSeconds: Number(marker.time) + Number(chart.META.offset ?? 0) / 1000 })),
      registeredGlobalEvents: editor.globalEvents.document.types.map(type => ({ id: type.id, name: type.name, source: type.source, columns: type.columns.map(column => ({ id: column.id, name: column.name })) })),
      features: commands.tools().map(tool => tool.function.name) };
  } }));
  const entities = chart => [
    ...(chart.markers ?? []).map((entry, index) => ({ kind: 'marker', index, name: String(entry.name), chartSeconds: Number(entry.time) })),
    ...(chart.judgeLineList ?? []).map((entry, index) => ({ kind: 'line', index, name: entry.Name || `Line ${index}` })),
    ...(chart.judgeLineGroup ?? []).map((name, index) => ({ kind: 'line_group', index, name: String(name) })),
    ...(chart.blockAreaList ?? []).map((entry, index) => ({ kind: 'noise', index, name: entry.Name || t('agent-editor.s28e7db2', { p0: index }), appearTime: entry.appearTime })),
    ...(chart.noiseAreaLayers ?? []).map((entry, index) => ({ kind: 'noise_layer', index, name: entry.name, id: entry.id })),
    ...[...editor.assets.keys()].map(name => ({ kind: 'asset', name })),
  ];
  register('editor_entities', t('agent-editor.sc9aacf7'), agentSchema({ kind: enumeration(['marker', 'line', 'line_group', 'noise', 'noise_layer', 'asset']), query: agentText, offset: agentIndex, limit: integer(1, 200) }), 'read', args => ({ apply: () => {
    const entries = entities(current()).filter(entry => (!args.kind || entry.kind === args.kind) && (!args.query || entry.name.toLowerCase().includes(args.query.toLowerCase())));
    return { total: entries.length, entries: entries.slice(args.offset ?? 0, (args.offset ?? 0) + (args.limit ?? 100)) };
  } }));
  register('editor_navigate', t('agent-editor.s1e06684'), agentSchema({ target: enumeration(['marker', 'line', 'noise', 'chart_seconds', 'music_seconds', 'global_beat', 'line_beat']), name: agentText, index: agentIndex, value: beatSchema, line: agentIndex }, ['target']), 'edit', args => {
    const chart = current(), session = editor.session, tempo = new TempoMap(chart.BPMList); let destination, selected;
    if (['marker', 'line', 'noise'].includes(args.target)) {
      const matches = entities(chart).filter(entry => entry.kind === args.target && (args.index !== undefined ? entry.index === args.index : entry.name.trim() === String(args.name ?? '').trim()));
      if (matches.length !== 1) throw Error(matches.length ? t('agent-editor.s9df9369') : t('agent-editor.s312f345'));
      selected = matches[0]; if (selected.kind === 'marker') destination = selected.chartSeconds;
      else if (selected.kind === 'noise') destination = tempo.seconds(selected.appearTime);
    } else {
      const value = agentBeat(args.value);
      destination = args.target === 'music_seconds' ? value - Number(chart.META.offset ?? 0) / 1000 : args.target === 'global_beat' ? tempo.seconds(value) : args.target === 'line_beat' ? tempo.seconds(value, chart.judgeLineList[args.line ?? session.lineIndex]?.bpmfactor ?? 1) : value;
    }
    if (destination !== undefined && !Number.isFinite(destination)) throw Error(t('agent-editor.s231fa43'));
    return { summary: t('agent-editor.s15e6593'), details: { target: args.target, name: selected?.name, index: selected?.index, chartSeconds: destination, changesChart: false }, valid: () => editor.session === session && editor.chart === chart,
      apply: () => {
        if (selected?.kind === 'line') editor.selectLine(selected.index);
        if (selected?.kind === 'noise') { editor.noiseEditor.open(); editor.noiseEditor.select(selected.index); }
        if (destination !== undefined) editor.seek(destination); editor.invalidate();
        return { chartSeconds: destination ?? editor.chartSeconds(), selected, changesChart: false };
      } };
  });
  register('chart_read', t('agent-editor.s2c6b00f'), agentSchema({ path: agentText, offset: agentIndex, limit: integer(1, 200), fields: array(agentText, 30) }), 'read', args => ({ apply: () => {
    const chart = current(), value = agentAt(chart, args.path ?? '');
    if (!args.path) return { revision: revision(chart), summary: agentChartSummary(chart), fields: Object.keys(chart) };
    const offset = args.offset ?? 0, limit = args.limit ?? 50;
    const items = Array.isArray(value) ? value.slice(offset, offset + limit).map((entry, index) => ({ index: offset + index, value: args.fields && entry && typeof entry === 'object' ? Object.fromEntries(args.fields.filter(key => Object.hasOwn(entry, key)).map(key => [key, entry[key]])) : entry })) : value;
    if (JSON.stringify(items).length > 50000) throw Error(t('agent-editor.s8ef8d90'));
    return { revision: revision(chart), path: args.path, total: Array.isArray(value) ? value.length : undefined, items: structuredClone(items) };
  } }));
  register('chart_query', t('agent-editor.s7c23109'), agentSchema({ path: agentText, start: beatSchema, end: beatSchema, startField: agentText, endField: agentText,
    where: array(agentSchema({ field: agentText, op: enumeration(['eq', 'ne', 'lt', 'lte', 'gt', 'gte', 'contains']), value: {} }, ['field', 'op', 'value']), 20), sort: agentText, offset: agentIndex, limit: integer(1, 200), fields: array(agentText, 30) }, ['path']), 'read', args => ({ apply: () => {
    const chart = current(), source = agentAt(chart, args.path); if (!Array.isArray(source)) throw Error(t('agent-editor.sfe523a4'));
    const field = (entry, key) => agentAt(entry, '/' + key.replaceAll('.', '/'));
    let entries = source.map((value, index) => ({ index, value })).filter(({ value }) => {
      for (const condition of args.where ?? []) {
        let actual; try { actual = field(value, condition.field); } catch { return false; }
        const expected = condition.value;
        if (!(condition.op === 'eq' ? JSON.stringify(actual) === JSON.stringify(expected) : condition.op === 'ne' ? JSON.stringify(actual) !== JSON.stringify(expected) : condition.op === 'lt' ? actual < expected : condition.op === 'lte' ? actual <= expected : condition.op === 'gt' ? actual > expected : condition.op === 'gte' ? actual >= expected : String(actual).includes(String(expected)))) return false;
      }
      if (args.start !== undefined || args.end !== undefined) {
        const start = agentBeat(field(value, args.startField ?? 'startTime')), end = agentBeat(field(value, args.endField ?? 'endTime'));
        if (args.start !== undefined && end < agentBeat(args.start) || args.end !== undefined && start >= agentBeat(args.end)) return false;
      }
      return true;
    });
    if (args.sort) entries.sort((left, right) => { const first = field(left.value, args.sort), last = field(right.value, args.sort); return (Array.isArray(first) ? beatValue(first) : first) - (Array.isArray(last) ? beatValue(last) : last); });
    const total = entries.length; entries = entries.slice(args.offset ?? 0, (args.offset ?? 0) + (args.limit ?? 50));
    if (args.fields) entries = entries.map(entry => ({ index: entry.index, value: Object.fromEntries(args.fields.map(key => { let value; try { value = field(entry.value, key); } catch {} return [key, value]; })) }));
    return { revision: revision(chart), total, entries: structuredClone(entries) };
  } }));
  register('chart_patch', t('agent-editor.s09ba43f'), agentSchema({ revision: agentText, label: agentText,
    operations: { type: 'array', minItems: 1, maxItems: 2000, items: agentSchema({ op: enumeration(['set', 'remove', 'insert', 'append']), path: agentText, value: {} }, ['op', 'path']) } }, ['revision', 'operations']), 'edit', args => {
    const chart = check(args); const next = agentChartPatch(chart, args.operations);
    const plan = prepareChart(args, chart, next.chart, args.label ?? t('agent-editor.s03b5f48'), { paths: args.operations.map(operation => ({ op: operation.op, path: operation.path, value: operation.value })) }, next.dangerous); return plan;
  });
  register('bpm_set', t('agent-editor.sf7d35ff'), agentSchema({ revision: agentText, entries: { type: 'array', minItems: 1, maxItems: 1000, items: agentSchema({ beat: beatSchema, bpm: number(0.001, 100000) }, ['beat', 'bpm']) }, source: agentText }, ['revision', 'entries']), 'edit', args => {
    const chart = check(args), entries = args.entries.map(entry => ({ startTime: fromNumber(agentBeat(entry.beat)), bpm: entry.bpm })).sort((left, right) => beatValue(left.startTime) - beatValue(right.startTime));
    if (entries.some((entry, index) => index && beatValue(entry.startTime) === beatValue(entries[index - 1].startTime))) throw Error(t('agent-editor.s00d868d'));
    return prepareChart(args, chart, { ...chart, BPMList: entries }, t('agent-editor.sdf04e79'), { entries, source: args.source ?? '' });
  });
  const numericScope = (value, index, count) => Object.assign(Object.create(null), Object.fromEntries(Object.entries(value).filter(([, entry]) => typeof entry === 'number')), { i: index, N: count, u: count <= 1 ? 0 : index / (count - 1), beat: value.startTime ? beatValue(value.startTime) : value.appearTime ? beatValue(value.appearTime) : 0 });
  const compileFields = expressions => {
    const entries = Object.entries(expressions); if (entries.length > 64) throw Error(t('agent-editor.s7d13db1'));
    return entries.map(([path, source]) => { if (typeof source !== 'string') throw Error(t('agent-editor.s12e963c')); const parts = agentPath(path); if (!parts.length) throw Error(t('agent-editor.s182d640')); return { path, parts, evaluate: compileExpression(source) }; });
  };
  const calculate = (item, expressions, scope) => {
    const operations = expressions.map(({ path, parts, evaluate }) => {
      const value = evaluate(scope); if (!Number.isFinite(value)) throw Error(t('agent-editor.saf546f8'));
      return { parts, value: /\/(?:startTime|endTime|appearTime|disappearTime)$/.test(path) ? fromNumber(value) : value };
    });
    let result = structuredClone(item);
    for (const operation of operations) {
      const parts = operation.parts;
      let parent = result;
      for (const key of parts.slice(0, -1)) { if (!parent || typeof parent !== 'object' || !Object.hasOwn(parent, key)) throw Error(t('agent-editor.scd77a25')); parent = parent[key]; }
      if (!parent || typeof parent !== 'object') throw Error(t('agent-editor.s49656c7'));
      Object.defineProperty(parent, parts.at(-1), { value: operation.value, enumerable: true, writable: true, configurable: true });
    }
    return result;
  };
  const expressionsSchema = { type: 'object', additionalProperties: true, description: t('agent-editor.s2111de4') };
  register('chart_transform', t('agent-editor.sb019088'), agentSchema({ revision: agentText, path: agentText, indices: array(agentIndex, 10000), where: agentText, expressions: expressionsSchema, remove: { type: 'boolean' }, label: agentText }, ['revision', 'path']), 'edit', args => {
    const chart = check(args), source = agentAt(chart, args.path); if (!Array.isArray(source)) throw Error(t('agent-editor.s0c785b9'));
    const indices = args.indices ? new Set(args.indices) : null, filter = args.where ? compileExpression(args.where) : () => true;
    const chosen = source.map((value, index) => ({ value, index })).filter(({ value, index }) => (!indices || indices.has(index)) && filter(numericScope(value, index, source.length)));
    if (!chosen.length) throw Error(t('agent-editor.sd5eaf43'));
    const expressions = compileFields(args.expressions ?? {});
    const targets = new Map(chosen.map(({ value, index }, offset) => [index, args.remove ? null : calculate(value, expressions, numericScope(value, offset, chosen.length))]));
    const changed = source.flatMap((value, index) => targets.has(index) ? args.remove ? [] : [targets.get(index)] : [value]);
    const next = agentChartPatch(chart, [{ op: 'set', path: args.path, value: changed }]);
    return prepareChart(args, chart, next.chart, args.label ?? t('agent-editor.sa5cd6b4'), { path: args.path, count: chosen.length, indices: chosen.map(entry => entry.index).slice(0, 200), expressions: args.expressions, remove: Boolean(args.remove) }, Boolean(args.remove && chosen.length > 50) || args.path === '/judgeLineList');
  });
  register('chart_generate', t('agent-editor.s718a867'), agentSchema({ revision: agentText, path: agentText, count: integer(1, 10000), template: { type: 'object' }, expressions: expressionsSchema, label: agentText }, ['revision', 'path', 'count', 'template']), 'edit', args => {
    const chart = check(args), expressions = compileFields(args.expressions ?? {}); const entries = Array.from({ length: args.count }, (unused, index) => calculate(args.template, expressions, numericScope(args.template, index, args.count)));
    const next = agentChartPatch(chart, [{ op: 'append', path: args.path, value: entries }]);
    return prepareChart(args, chart, next.chart, args.label ?? t('agent-editor.sffd802a'), { path: args.path, count: args.count, template: args.template, expressions: args.expressions }, args.path === '/judgeLineList');
  });
  register('line_manage', t('agent-editor.sd99859f'), agentSchema({ revision: agentText, action: enumeration(['ensure', 'duplicate', 'delete', 'reorder', 'parent']), line: agentIndex, target: agentIndex, count: integer(1, 10000), parent: integer(-1, 10000) }, ['revision', 'action']), 'edit', args => {
    const chart = check(args); let next;
    if (args.action === 'ensure') next = ensureLineCount(chart, args.count, editor.session.lineEventDefaults);
    else if (args.action === 'duplicate') next = duplicateLine(chart, args.line);
    else if (args.action === 'delete') { if (chart.judgeLineList.length <= 1) throw Error(t('agent-editor.sc8b778e')); next = deleteLine(chart, args.line); }
    else if (args.action === 'reorder') next = reorderLine(chart, args.line, args.target);
    else next = setLineParent(chart, args.line, args.parent);
    return prepareChart(args, chart, next, t('agent-editor.s96640ef') + args.action, { line: args.line, target: args.target, count: args.count, parent: args.parent }, args.action === 'delete');
  });
  register('event_shake', t('agent-editor.s2401611'), agentSchema({ revision: agentText, line: agentIndex, track: enumeration(['moveXEvents', 'moveYEvents', 'rotateEvents']), layer: integer(0, 3), start: beatSchema, duration: beatSchema, amplitude: number(0, 100000), cycles: number(0.01, 1000), segmentsPerCycle: integer(4, 128), decayEasing: integer(1, 29), baseline: agentNumber, replace: { type: 'boolean' } }, ['revision', 'line', 'track', 'start', 'duration', 'amplitude', 'cycles', 'decayEasing']), 'edit', args => {
    const chart = check(args), result = agentShake(chart, args); return prepareChart(args, chart, result.chart, t('agent-editor.s528ed4d'), { ...args, events: result.count }, Boolean(args.replace));
  });
  register('noise_circle', t('agent-editor.sb840325'), agentSchema({ revision: agentText, start: beatSchema, duration: beatSchema, centerX: agentNumber, centerY: agentNumber, radius: number(0.01, 100000), width: number(0.01, 100000), height: number(0.01, 100000), turns: number(0.01, 100), phase: agentNumber, clockwise: { type: 'boolean' }, samples: integer(8, 512), active: { type: 'boolean' }, invert: { type: 'boolean' }, layerId: agentText, name: agentText }, ['revision', 'start', 'duration', 'centerX', 'centerY', 'radius', 'width', 'height']), 'edit', args => {
    const chart = check(args), result = agentCircle(chart, args); return prepareChart(args, chart, result.chart, t('agent-editor.sc650215'), { ...args, index: result.index, events: result.count });
  });
  const audioAnalysis = async (args, signal) => {
    const chart = current(), line = chart.judgeLineList[args.line ?? editor.session.lineIndex]; if (!line) throw Error(t('agent-editor.s181a573'));
    const tempo = new TempoMap(chart.BPMList), offset = Number(chart.META.offset ?? 0) / 1000, start = agentBeat(args.start), end = agentBeat(args.end), factor = line.bpmfactor ?? 1;
    const source = editor.audioSources ? await editor.audioSources.resolveForAnalysis(args.source, signal) : { id: 'original', name: t('agent-editor.sdd1d7cc'), buffer: editor.audio.buffer, offsetSeconds: 0 };
    if (!editor.audioSources && args.source && args.source !== 'original') throw Error(t('agent-editor.sc0bedbc'));
    const sourceOffset = Number(source.offsetSeconds ?? 0), startSeconds = tempo.seconds(start, factor) + offset, endSeconds = tempo.seconds(end, factor) + offset;
    const range = editor.audioSources ? editor.audioSources.range(source, startSeconds, endSeconds) : { startSeconds, endSeconds };
    const options = { startSeconds: range.startSeconds, endSeconds: range.endSeconds, detector: args.detector, sensitivity: args.sensitivity, minGap: args.minGap, focus: args.focus, minConfidence: args.minConfidence, mapperModel: args.mapperModel, mapperMode: args.mapperMode, difficulty: args.difficulty, seed: args.seed, useChartTiming: args.useChartTiming, timing: mapperTiming(chart, sourceOffset) };
    if (!editor.audioOnsets && args.detector && args.detector !== 'flux') throw Error(t('agent-editor.sc6464bc'));
    const result = editor.audioOnsets ? await editor.audioOnsets.analyze(source.buffer, options, signal) : await analyzeAgentAudio(source.buffer, { ...options, signal });
    return { ...result, startSeconds, endSeconds, sourceStartSeconds: result.startSeconds, sourceEndSeconds: result.endSeconds, source: { id: source.id, ids: source.ids ?? [source.id], name: source.name, offsetSeconds: 0 }, peaks: result.peaks.map(peak => ({ ...peak, seconds: peak.seconds, beat: tempo.beat(peak.seconds - offset, factor) })), start, end };
  };
  const audioSchema = { line: agentIndex, start: beatSchema, end: beatSchema, source: { ...agentText, description: t('agent-editor.s614a3b0') }, detector: { ...enumeration(['mapper', 'ddc', 'flux']), description: t('agent-editor.s8ddad36') }, mapperModel: enumeration(['v32', 'v32-mini']), mapperMode: { type: 'integer', enum: [0, 3], description: t('agent-editor.s7a96586') }, difficulty: { ...number(.1, 15), description: t('agent-editor.s80a8d5d') }, seed: integer(0, 0xffffffff), useChartTiming: { type: 'boolean', description: t('agent-editor.sea6700e') }, sensitivity: { ...number(0.1, 10), description: t('agent-editor.sa343a1f') }, minGap: number(0.02, 2), focus: { ...enumeration(['all', 'percussion', 'low', 'melody']), description: t('agent-editor.se976f3a') }, minConfidence: { ...number(0, 1), description: t('agent-editor.se3ac0ce') } };
  register('audio_sources', t('agent-editor.s178a7f2'), agentSchema({ status: { type: 'boolean' } }), 'read', args => ({ apply: async () => ({ sources: editor.audioSources?.list() ?? [{ id: 'original', name: t('agent-editor.sdd1d7cc') }], selection: editor.audioSources?.selectedIds ?? ['original'], separation: args.status ? editor.audioSources?.separator.bridge ? await editor.audioSources.separator.call('probe') : { ready: false, message: t('agent-editor.s5e2a3d0') } : undefined }) }));
  register('audio_separate', t('agent-editor.s293f2fc'), agentSchema({ model: enumeration(['htdemucs', 'htdemucs_6s']), device: enumeration(['auto', 'cpu', 'cuda']), shifts: integer(0, 4) }), 'read', (args, { signal }) => ({ apply: async () => { if (!editor.audioSources) throw Error(t('agent-editor.s7fd0b91')); return editor.audioSources.separate(args, signal); } }));
  register('audio_source', t('agent-editor.sb33b93b'), agentSchema({ action: enumeration(['select', 'remove']), source: agentText, sources: array(agentText) }, ['action']), 'edit', args => {
    const sources = editor.audioSources; if (!sources) throw Error(t('agent-editor.scc76ec4')); const revision = sources.revision, original = editor.audio.buffer;
    if (args.action === 'remove' ? !args.source || args.sources : Boolean(args.source) === Boolean(args.sources?.length)) throw Error(t('agent-editor.s1f63ce2'));
    for (const id of args.sources ?? [args.source]) sources.entry(id);
    return { summary: t('agent-editor.s405820e') + args.action, details: args, valid: () => sources.revision === revision && editor.audio.buffer === original, apply: async () => { if (args.action === 'select') await sources.select(args.sources ?? args.source); else await sources.remove(args.source); return { sources: sources.list(), selection: sources.selectedIds, chartUnchanged: true }; } };
  });
  register('audio_detector', t('agent-editor.sd53908e'), agentSchema({ status: { type: 'boolean' }, detector: enumeration(['mapper', 'ddc', 'flux']) }), 'read', args => ({ apply: async () => { const onsets = editor.audioOnsets, selected = args.detector ?? onsets?.options.detector, backend = selected === 'mapper' ? onsets?.mapper : selected === 'ddc' ? onsets?.detector : null; return { options: onsets?.options ?? { detector: 'flux' }, detectors: [{ id: 'mapper', available: Boolean(onsets?.mapper.bridge) }, { id: 'ddc', available: Boolean(onsets?.detector.bridge) }, { id: 'flux', available: true }], environment: args.status && backend?.bridge ? await backend.call('probe') : undefined }; } }));
  register('audio_analyze', t('agent-editor.s58b6271'), agentSchema(audioSchema, ['start', 'end']), 'read', async (args, { signal }) => {
    const result = await audioAnalysis(args, signal); return { apply: () => result };
  });
  register('audio_taps', t('agent-editor.sc789bcf'), agentSchema({ ...audioSchema, revision: agentText, division: integer(1, 192), positionX: agentNumber, quantization: enumeration(['adaptive', 'nearest', 'none']), quantizeTolerance: number(0, .5) }, ['revision', 'line', 'start', 'end', 'division']), 'edit', async (args, { signal }) => {
    const chart = check(args), originalAudio = editor.audio.buffer, sourcesRevision = editor.audioSources?.revision, result = await audioAnalysis(args, signal); const division = args.division;
    const selected = quantizeAgentPeaks(result.peaks, { division, start: result.start, end: result.end, mode: args.quantization ?? 'adaptive', tolerance: args.quantizeTolerance ?? .2 });
    const beats = selected.map(peak => peak.beat);
    const next = agentTapNotes(chart, args.line, beats, args.positionX ?? 0);
    if (!next.count) throw Error(result.detector === 'mapper' ? t('agent-editor.s2564c2e') : t('agent-editor.sace83fe'));
    const plan = prepareChart(args, chart, next.chart, t('agent-editor.s0f318c0'), { line: args.line, source: result.source, start: result.start, end: result.end, beats, added: next.count, skipped: beats.length - next.count, method: result.method, model: result.model, statistics: result.statistics, focus: result.focus, quantization: args.quantization ?? 'adaptive', candidates: selected }), valid = plan.valid;
    return { ...plan, valid: () => valid() && editor.audio.buffer === originalAudio && editor.audioSources?.revision === sourcesRevision };
  });
  register('diagnose', t('agent-editor.sb162aaf'), agentSchema({ offset: agentIndex, limit: integer(1, 200) }), 'read', args => ({ apply: async () => { const issues = editor.diagnostics ? (await editor.diagnostics.waitBasic(), editor.diagnostics.issues) : diagnose(current()); return { total: issues.length, issues: issues.slice(args.offset ?? 0, (args.offset ?? 0) + (args.limit ?? 50)) }; } }));
  register('editor_view', t('agent-editor.scec3192'), agentSchema({ line: agentIndex, layer: integer(0, 3), pane: agentText, playback: enumeration(['play', 'pause']), preview: { type: 'boolean' }, notes: array(agentIndex), events: array(agentText), noise: agentIndex }), 'edit', args => {
    const session = editor.session;
    return { summary: t('agent-editor.s76a018a'), details: args, valid: () => editor.session === session, apply: async () => {
      if (args.line !== undefined) { if (!editor.chart.judgeLineList[args.line]) throw Error(t('agent-editor.s181a573')); editor.selectLine(args.line); }
      if (args.layer !== undefined) editor.timeline.layer = session.eventLayer = args.layer;
      if (args.notes) { session.selection = new Set(args.notes); session.focus = 'notes'; }
      if (args.events) { session.eventSelection = new Set(args.events); session.focus = 'events'; }
      if (args.noise !== undefined) { if (!editor.chart.blockAreaList?.[args.noise]) throw Error(t('agent-editor.s2655e17')); editor.noiseEditor.open(); editor.noiseEditor.select(args.noise); }
      if (args.pane !== undefined) { if (args.pane === 'ai' || !editor.agent.allowedPanes().includes(args.pane)) throw Error(t('agent-editor.sa8f34da')); editor.activatePane(args.pane); }
      if (args.preview !== undefined) editor.setPreview(args.preview);
      if (args.playback === 'play') await editor.play(); else if (args.playback === 'pause') editor.pause();
      session.notify(); editor.invalidate(); return { applied: args };
    } };
  });
  register('history', t('agent-editor.s264484a'), agentSchema({ action: enumeration(['list', 'undo', 'redo']) }, ['action']), 'read', args => { const session = editor.session, chart = editor.chart; return { summary: t('agent-editor.s3ec9c63') + args.action, access: args.action === 'list' ? 'read' : 'edit', valid: () => editor.session === session && editor.chart === chart, apply: () => {
    if (args.action !== 'list') editor.session.travel(args.action);
    return { revision: revision(editor.chart), undo: editor.session.history.undoStack.map(entry => entry.label).slice(-30), redo: editor.session.history.redoStack.map(entry => entry.label).slice(-30) };
  } }; });
  register('assets_list', t('agent-editor.sa22c8da'), agentSchema({ query: agentText, offset: agentIndex, limit: integer(1, 200) }), 'read', args => ({ apply: () => { const files = [...editor.assets].filter(([path]) => !args.query || path.includes(args.query)); return { total: files.length, files: files.slice(args.offset ?? 0, (args.offset ?? 0) + (args.limit ?? 100)).map(([path, bytes]) => ({ path, bytes: bytes.byteLength })) }; } }));
  register('asset_read', t('agent-editor.s4fe544a'), agentSchema({ path: agentText, format: enumeration(['text', 'image', 'base64', 'info']), offset: agentIndex, limit: integer(1, 40000) }, ['path', 'format']), 'read', args => ({ apply: async () => {
    const source = editor.assets.get(pathOf(args.path)); if (!source) throw Error(t('agent-editor.sfba60f1')); const bytes = contentBytes(source);
    if (args.format === 'info') return { path: args.path, bytes: bytes.length };
    if (args.format === 'image') {
      if (!editor.agent.capabilities().images) throw Error(t('agent-editor.sc9bbaee'));
      if (!/\.(png|jpe?g|webp|gif)$/i.test(args.path) || bytes.length > 5 * 1024 * 1024) throw Error(t('agent-editor.sc3017be'));
      const mime = /\.png$/i.test(args.path) ? 'image/png' : /\.webp$/i.test(args.path) ? 'image/webp' : /\.gif$/i.test(args.path) ? 'image/gif' : 'image/jpeg';
      const image = await editor.agent.dataUrl(new Blob([bytes], { type: mime })); return { path: args.path, image };
    }
    if (args.format === 'text') { if (bytes.length > 4 * 1024 * 1024) throw Error(t('agent-editor.s9abf8d7')); const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes); return { total: text.length, offset: args.offset ?? 0, text: text.slice(args.offset ?? 0, (args.offset ?? 0) + (args.limit ?? 20000)) }; }
    if (bytes.length > 24000) throw Error(t('agent-editor.sb6d781b'));
    return { base64: btoa(String.fromCharCode(...bytes)) };
  } }));
  register('asset_write', t('agent-editor.sa2828c5'), agentSchema({ path: agentText, action: enumeration(['write', 'delete']), text: agentText, base64: { type: 'string', maxLength: 1500000 }, binding: enumeration(['song', 'background']) }, ['path', 'action']), 'dangerous', args => {
    const path = pathOf(args.path), session = editor.session, previous = editor.assets.get(path);
    let bytes;
    if (args.action === 'write') {
      if ((args.text !== undefined) === (args.base64 !== undefined)) throw Error(t('agent-editor.sb7a2276'));
      bytes = args.text !== undefined ? new TextEncoder().encode(args.text) : Uint8Array.from(atob(args.base64), value => value.charCodeAt(0));
    }
    return { summary: t('agent-editor.sabb9f6a', { p0: args.action === 'delete' ? t('agent-editor.s3755f56') : previous ? t('agent-editor.s855241c') : t('agent-editor.s2cd9e6c'), p1: path }), details: { path, bytes: bytes?.length, binding: args.binding, undoable: false, needsSave: true }, valid: () => editor.session === session && editor.assets.get(path) === previous,
      apply: () => editor.agent.writeAsset(path, bytes, args.binding) };
  });
  register('ui_inspect', t('agent-editor.s379345e'), agentSchema({ query: agentText }), 'read', args => ({ apply: () => controls.inspect(args.query) }));
  register('ui_action', t('agent-editor.s6030cfc'), agentSchema({ handle: agentText, action: enumeration(['click', 'set', 'pointer', 'key']), value: {}, x: agentNumber, y: agentNumber, button: integer(0, 2), key: agentText, ctrl: { type: 'boolean' }, shift: { type: 'boolean' }, alt: { type: 'boolean' }, meta: { type: 'boolean' } }, ['handle', 'action']), 'edit', args => controls.prepare(args));
  register('preview_capture', t('agent-editor.sac40096'), agentSchema(), 'read', () => ({ apply: async () => {
    if (!editor.agent.capabilities().images) throw Error(t('agent-editor.sb63e55b')); return { image: await capture() };
  } }));
  register('editor_capture', t('agent-editor.s862b27b'), agentSchema({ target: enumeration(['editor', 'timeline', 'panel', 'dialog', 'control']), handle: agentText }), 'read', (args, { signal }) => ({ apply: async () => {
    if (!editor.agent.capabilities().images) throw Error(t('agent-editor.sb63e55b'));
    if (!captureEditor) throw Error(t('agent-editor.sa2a28ec'));
    return captureEditor(args, signal);
  } }));
  register('web_search', t('agent-editor.s3b613fc'), agentSchema({ query: { type: 'string', maxLength: 128 } }, ['query']), 'read', (args, { signal }) => ({ apply: () => network('search', args.query, signal) }));
  register('web_fetch', t('agent-editor.sb66208e'), agentSchema({ url: agentText }, ['url']), 'read', (args, { signal }) => {
    const url = new URL(args.url); if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || /^(localhost|127\.|0\.|10\.|192\.168\.|169\.254\.|172\.(1[6-9]|2\d|3[01])\.|\[|.*\.local$)/i.test(url.hostname)) throw Error(t('agent-editor.s1c0b848'));
    return { apply: () => network('fetch', url.href, signal) };
  });
  register('ask_user', t('agent-editor.sbd55435'), agentSchema({ question: agentText, choices: array({ ...agentText, description: t('agent-editor.s4f75e82') }, 8) }, ['question']), 'read', (args, { signal }) => ({ apply: () => ask(args, signal) }));
  register('project_save', t('agent-editor.sc0775af'), agentSchema(), 'dangerous', () => { const session = editor.session; return { summary: t('agent-editor.sa01239d'), valid: () => editor.session === session, apply: async () => { await editor.save(); return { saved: !editor.dirty, needsSave: editor.dirty }; } }; });
  registerAgentChartTools(commands, editor, { ...patterns, current, check, prepareChart });
  registerAgentWorkspaceTools(commands, editor, { current, check, prependSilence });
  registerAgentScriptingTools(commands, editor, { check, prepareChart });
  const files = registerAgentFileTools(commands, editor, workspace);
  registerAgentComponentTools(commands, editor, files);
  return { revision: () => revision(editor.chart), summary: () => {
    const chart = editor.chart, tempo = new TempoMap(chart.BPMList), seconds = editor.chartSeconds();
    const summary = agentChartSummary(chart); summary.bpm = summary.bpm.slice(0, 12); summary.noiseLayers = summary.noiseLayers.map(layer => ({ id: layer.id, name: layer.name, skin: layer.skin }));
    return { ...summary, revision: revision(chart), workspace: { currentLine: editor.session.lineIndex, currentLineName: chart.judgeLineList[editor.session.lineIndex]?.Name, currentLayer: editor.timeline.layer, pane: editor.activePane, selectedNotes: [...editor.session.selection].slice(0, 20), selectedEvents: [...editor.session.eventSelection].slice(0, 20), selectedNoteCount: editor.session.selection.size, selectedEventCount: editor.session.eventSelection.size },
      timeBases: { chartSeconds: seconds, musicSeconds: seconds + Number(chart.META.offset ?? 0) / 1000, globalBeat: tempo.beat(seconds), lineBeat: tempo.beat(seconds, chart.judgeLineList[editor.session.lineIndex]?.bpmfactor ?? 1) },
      namedEntities: { markers: (chart.markers ?? []).slice(0, 20).map((marker, index) => ({ kind: 'marker', index, name: String(marker.name), chartSeconds: Number(marker.time) })), lines: chart.judgeLineList.slice(0, 20).map((line, index) => ({ kind: 'line', index, name: line.Name })), groups: (chart.judgeLineGroup ?? []).slice(0, 20), noiseLayers: summary.noiseLayers.slice(0, 20) },
      semantics: t('agent-editor.sd2104e9') };
  } };
}
