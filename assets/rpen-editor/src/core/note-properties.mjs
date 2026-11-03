import { t } from '../i18n.mjs';

export const DEFAULT_NOTE_TINT = Object.freeze([255, 255, 255]);
export const DEFAULT_HIT_TINT = Object.freeze([255, 236, 160]);

export function noteColor(note, field) {
  const fallback = field === 'tint' ? DEFAULT_NOTE_TINT : DEFAULT_HIT_TINT;
  const value = field === 'tint' ? note.tint ?? note.color : note.tintHitEffects;
  return Array.isArray(value) && value.length === 3 && value.every(Number.isFinite) ? value.map(channel => Math.max(0, Math.min(255, Math.round(channel)))) : [...fallback];
}

export function parseNoteColor(source) {
  const values = String(source).trim().split(/[\s,]+/).map(Number);
  if (values.length !== 3 || values.some(value => !Number.isInteger(value) || value < 0 || value > 255)) throw Error(t('note-properties.invalidColor'));
  return values;
}

export function setNoteHitSound(note, value) {
  const next = { ...note }; delete next.hitSound;
  if (value) next.hitsound = value; else delete next.hitsound;
  return next;
}
