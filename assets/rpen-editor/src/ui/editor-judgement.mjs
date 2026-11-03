import { originalEditorContext } from './editor-skin-canvas.mjs';
import { DEFAULT_EDITOR_PREFERENCES } from '../core/default-settings.mjs';

export const editorJudgementWidth = (settings, referenceWidth) => Math.max(0.5, (settings.judgementWidth ?? DEFAULT_EDITOR_PREFERENCES.judgementWidth) * 2 * referenceWidth / 1920);

export function drawEditorJudgement(context, segments, color = '#ffffff', width = 2, alpha = DEFAULT_EDITOR_PREFERENCES.judgementAlpha) {
  context = originalEditorContext(context);
  context.save(); context.globalAlpha = Math.max(0, Math.min(1, alpha)); context.globalCompositeOperation = 'source-over';
  context.strokeStyle = color; context.lineWidth = width; context.setLineDash([]); context.beginPath();
  for (const segment of segments) { context.moveTo(segment.left, segment.y); context.lineTo(segment.right, segment.y); }
  context.stroke(); context.restore();
}
