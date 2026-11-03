import { t } from '../i18n.mjs';
export const DEFAULT_BEZIER_EXPORT_MARGIN = 0.05;

export function bezierExportMargin(value = DEFAULT_BEZIER_EXPORT_MARGIN) {
  if (!Number.isFinite(value) || value < 0.001 || value > 0.25) throw Error(t('bezier-compat.s9e4b6f8'));
  return value;
}

export function clampExportBezierEvent(event, margin = DEFAULT_BEZIER_EXPORT_MARGIN) {
  const points = event?.bezierPoints;
  if (!event?.bezier || !Array.isArray(points) || points.length !== 4 || !points.every(Number.isFinite)) return event;
  const first = points[0], second = points[2];
  const risky = first < 0 || first > 1 || second < 0 || second > 1 || first < margin && second < margin || first > 1 - margin && second > 1 - margin || first > 1 - margin && second < margin;
  if (!risky) return event;
  const clamp = value => Math.max(margin, Math.min(1 - margin, value));
  return { ...event, bezierPoints: [clamp(first), points[1], clamp(second), points[3]] };
}
