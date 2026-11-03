import { shortcutKey } from '../core/preferences.mjs';

export function isTextEntry(target) {
  if (target?.isContentEditable || target?.closest?.('textarea,[contenteditable="true"],[role="textbox"]')) return true;
  const input = target?.closest?.('input');
  return Boolean(input && !['button', 'submit', 'reset', 'checkbox', 'radio', 'range', 'file', 'color'].includes(input.type));
}

export function isPlaybackSpace(event) {
  return shortcutKey(event) === 'SPACE' && !event.ctrlKey && !event.metaKey && !event.altKey && !isTextEntry(event.target);
}
export function isTypingText(target) {
  if (target?.isContentEditable || target?.closest?.('textarea,[contenteditable="true"],[role="textbox"]')) return true;
  const input = target?.closest?.('input');
  return Boolean(input && ['text', 'search', 'url', 'email', 'password', 'tel'].includes(input.type));
}

export function usesNativeTextShortcut(event) {
  if (event.defaultPrevented || event.isComposing || event.keyCode === 229) return true;
  if (isTypingText(event.target)) return true;
  if (!isTextEntry(event.target)) return false;
  if (event.target?.closest?.('input')?.type !== 'number') return true;
  if (event.ctrlKey || event.metaKey || event.altKey) return true;
  return /^[0-9eE.,+-]$/.test(event.key) || ['Backspace', 'Delete', 'Insert', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown', 'Tab', 'Enter', 'Escape', 'Dead', 'Process'].includes(event.key);
}

export function releaseShortcutFocus(target) {
  if (!isTypingText(target) && target?.closest?.('input,select,button')) target.blur?.();
}
