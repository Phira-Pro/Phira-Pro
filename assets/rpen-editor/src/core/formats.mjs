import { parseChart } from './chart.mjs';
import { integerChartAlpha } from './alpha-values.mjs';
import { parseLegacyChart } from './legacy-chart.mjs';
import { parsePecChart } from './pec-chart.mjs';
import { parseOfficialChart } from './official-chart.mjs';
import { emitModHook } from './mod-hooks.mjs';

export function parseDocument(text) {
  const custom=emitModHook('chart:parse',{text,chart:null});if(custom.chart)return custom.chart;text=custom.text;
  const trimmed = text.replace(/^\uFEFF/, '').trimStart();
  if (trimmed.startsWith('{')) {
    const candidate = JSON.parse(trimmed);
    return emitModHook('chart:parsed',{chart:candidate.formatVersion && !candidate.META ? integerChartAlpha(parseOfficialChart(candidate)) : parseChart(trimmed)}).chart;
  }
  const bpm = trimmed.split(/\r?\n/).find(row => /^bp\s/.test(row.trim()));
  return emitModHook('chart:parsed',{chart:integerChartAlpha(bpm?.trim().split(/\s+/).length === 5 ? parseLegacyChart(trimmed) : parsePecChart(trimmed))}).chart;
}
