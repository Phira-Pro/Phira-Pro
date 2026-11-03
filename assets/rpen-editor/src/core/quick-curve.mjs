import { beatValue } from './beat.mjs';
import { generateCurveNotes } from './curve-notes.mjs';
import { noteIsAbove } from './chart.mjs';
import { createPlacedNote } from './placement-defaults.mjs';
import { t } from '../i18n.mjs';

export function quickCurveNotes(source, endBeat, endX, { division = 4, density = 1, easingType = 1, noteDefaults } = {}) {
  const original = beatValue(source.startTime), finish = beatValue(endBeat);
  if (original === finish && source.positionX === endX) return [];
  const reverse = finish < original;
  const notes = generateCurveNotes({ startTime: reverse ? endBeat : source.startTime, endTime: reverse ? source.startTime : endBeat,
    startX: reverse ? endX : source.positionX, endX: reverse ? source.positionX : endX, type: 4, division, density, easingType, noteDefaults });
  const side = noteIsAbove(source) ? 1 : 2;
  return [...notes, { ...createPlacedNote(4, finish, endX, undefined, noteDefaults), startTime: [...endBeat], endTime: [...endBeat] }].map(note => ({ ...note, above: side }));
}

export function quickCurveChart(chart, lineIndex, notes, endLineIndex = lineIndex) {
  if (!notes.length) return chart;
  const lines = [...chart.judgeLineList], interior = notes.slice(0, -1), endpoint = notes.at(-1);
  for (const index of new Set([lineIndex, endLineIndex])) {
    const added = [...(index === lineIndex ? interior : []), ...(index === endLineIndex ? [endpoint] : [])];
    const source = lines[index]; if (!source) throw Error(t('curve-dialog.sb9277d1'));
    const next = [...source.notes, ...added]; lines[index] = { ...source, notes: next, numOfNotes: next.length };
  }
  return { ...chart, judgeLineList: lines };
}
