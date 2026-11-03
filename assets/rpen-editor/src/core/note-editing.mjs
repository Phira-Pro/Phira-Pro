import { beatValue } from './beat.mjs';

export function visibleBeats(note, tempo, factor = 1) {
  const start = beatValue(note.startTime);
  return start - tempo.beat(tempo.seconds(start, factor) - (note.visibleTime ?? 999999), factor);
}

export function visibleSeconds(note, beats, tempo, factor = 1) {
  const start = beatValue(note.startTime);
  return tempo.seconds(start, factor) - tempo.seconds(start - Math.max(0, beats), factor);
}

export const EVENT_WHEEL_STEPS = { moveXEvents: 5, moveYEvents: 5, rotateEvents: 0.5, alphaEvents: 5, speedEvents: 0.1, scaleXEvents: 0.1, scaleYEvents: 0.1 };
