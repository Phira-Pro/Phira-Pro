import { beatValue, fromNumber } from './beat.mjs';

export function dragEventEndpoint(event, field, beat) {
  let startTime = field === 'startTime' ? fromNumber(beat) : event.startTime;
  let endTime = field === 'endTime' ? fromNumber(beat) : event.endTime;
  if (beatValue(startTime) > beatValue(endTime)) [startTime, endTime] = [endTime, startTime];
  if (beatValue(startTime) === beatValue(endTime)) endTime = fromNumber(beatValue(startTime) + 1 / (startTime[2] * 5));
  return { ...event, startTime, endTime };
}
