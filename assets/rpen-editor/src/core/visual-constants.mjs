export const DEFAULT_LINE_WIDTH = 4000;
export const DEFAULT_LINE_HEIGHT = 5;
export const HIT_FRAME_SECONDS = 0.015;
export const HIT_FRAME_COUNT = 31;
export const HIT_DURATION = HIT_FRAME_SECONDS * HIT_FRAME_COUNT;
export function hitFrame(age) { return age < 0 || age >= HIT_DURATION ? null : Math.min(HIT_FRAME_COUNT - 1, Math.floor(age / HIT_FRAME_SECONDS)) + 1; }
