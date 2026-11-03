import { parseShortcut } from './shortcut-spec.mjs';
import { shortcutKey } from './preferences.mjs';

export const DICTATION_DEFAULTS = Object.freeze({ enabled: true, shortcut: 'TILDE', maximumSeconds: 120 });
const digits = Object.freeze({ 零: 0, 〇: 0, 一: 1, 幺: 1, 二: 2, 两: 2, 三: 3, 四: 4, 五: 5, 六: 6, 七: 7, 八: 8, 九: 9 });
const units = Object.freeze({ 十: 10, 百: 100, 千: 1000, 万: 10000, 亿: 100000000 });
const englishDigits = Object.freeze({ zero: 0, one: 1, two: 2, three: 3, four: 4, five: 5, six: 6, seven: 7, eight: 8, nine: 9 });
const englishSmall = Object.freeze({ ...englishDigits, ten: 10, eleven: 11, twelve: 12, thirteen: 13, fourteen: 14, fifteen: 15, sixteen: 16, seventeen: 17, eighteen: 18, nineteen: 19, twenty: 20, thirty: 30, forty: 40, fifty: 50, sixty: 60, seventy: 70, eighty: 80, ninety: 90 });
const englishScales = Object.freeze({ thousand: 1000, million: 1000000, billion: 1000000000, trillion: 1000000000000 });
const englishSigns = Object.freeze({ minus: -1, negative: -1, plus: 1, positive: 1 });
const englishWords = [...Object.keys(englishSmall), ...Object.keys(englishScales), ...Object.keys(englishSigns), 'hundred', 'point', 'dot'];
const englishNumberAtom = `(?:a(?=[\\s-]+(?:hundred|thousand|million|billion|trillion)\\b)|${englishWords.join('|')}|\\d+(?:\\.\\d+)?)`;
const englishNumberPattern = new RegExp(`\\b${englishNumberAtom}(?:[\\s-]+(?:${englishNumberAtom}|and(?=[\\s-]+${englishNumberAtom}\\b)))*\\b`, 'gi');

function englishGroup(tokens) {
  if (!tokens.length) return null;
  if (tokens.length === 1) {
    if (tokens[0] === 'hundred') return 100;
    if (Object.hasOwn(englishSmall, tokens[0])) return englishSmall[tokens[0]];
    return /^\d+(?:\.\d+)?$/.test(tokens[0]) ? Number(tokens[0]) : null;
  }
  const hundred = tokens.indexOf('hundred');
  if (hundred >= 0) {
    if (hundred > 1 || tokens.lastIndexOf('hundred') !== hundred) return null;
    const leading = hundred === 0 || tokens[0] === 'a' ? 1 : englishGroup(tokens.slice(0, hundred));
    if (!Number.isInteger(leading) || leading <= 0 || leading >= 100) return null;
    const rest = tokens.slice(hundred + 1);
    if (rest[0] === 'and') { rest.shift(); if (!rest.length) return null; }
    const trailing = rest.length ? englishGroup(rest) : 0;
    return trailing !== null && trailing >= 0 && trailing < 100 ? leading * 100 + trailing : null;
  }
  if (tokens.length === 2 && englishSmall[tokens[0]] >= 20 && englishDigits[tokens[1]] > 0) return englishSmall[tokens[0]] + englishDigits[tokens[1]];
  if (tokens.every(token => Object.hasOwn(englishDigits, token) || /^\d$/.test(token))) return Number(tokens.map(token => englishDigits[token] ?? token).join(''));
  return null;
}

export function englishNumber(source) {
  const tokens = String(source).toLowerCase().trim().split(/[\s-]+/);
  let sign = 1;
  if (Object.hasOwn(englishSigns, tokens[0])) sign = englishSigns[tokens.shift()];
  const separator = tokens.findIndex(token => token === 'point' || token === 'dot');
  const integer = separator < 0 ? tokens : tokens.slice(0, separator);
  let total = 0, group = [], previousScale = Infinity;
  for (const token of integer) {
    if (!Object.hasOwn(englishScales, token)) { group.push(token); continue; }
    const scale = englishScales[token];
    if (scale >= previousScale) return null;
    if (group[0] === 'and' && total > 0) group.shift();
    const implicitOne = group.length === 1 && group[0] === 'a' || !group.length && total === 0;
    const value = implicitOne ? 1 : englishGroup(group);
    if (value === null || !Number.isInteger(value) || value <= 0 || !Number.isSafeInteger(total + value * scale)) return null;
    total += value * scale; previousScale = scale; group = [];
  }
  if (group[0] === 'and' && total > 0) { group.shift(); if (!group.length) return null; }
  const tail = group.length ? englishGroup(group) : (total || separator >= 0 ? 0 : null);
  if (tail === null || !Number.isFinite(total + tail) || total && tail >= previousScale) return null;
  total += tail;
  if (separator >= 0) {
    const fraction = tokens.slice(separator + 1);
    if (!fraction.length || !fraction.every(token => Object.hasOwn(englishDigits, token) || /^\d+$/.test(token)) || !Number.isInteger(total)) return null;
    total = Number(String(total) + '.' + fraction.map(token => englishDigits[token] ?? token).join(''));
  }
  return Number.isFinite(total) && Math.abs(total) <= Number.MAX_SAFE_INTEGER ? sign * total : null;
}

export function englishSpokenNumbers(source) {
  return String(source).replace(englishNumberPattern, token => {
    const value = englishNumber(token);
    return value === null ? token : String(value);
  });
}

export function chineseNumber(source) {
  const text = String(source).replaceAll(' ', '');
  if (!text || !/^[零〇一幺二两三四五六七八九十百千万亿0-9]+$/.test(text)) return null;
  if (!/[十百千万亿]/.test(text)) return Number([...text].map(character => digits[character] ?? character).join(''));
  let total = 0, section = 0, current = '';
  for (const character of text) {
    if (Object.hasOwn(digits, character) || /[0-9]/.test(character)) { current += String(digits[character] ?? character); continue; }
    const unit = units[character];
    if (unit < 10000) section += Number(current || '1') * unit;
    else if (unit === 10000) { total += (section + Number(current || '0')) * unit; section = 0; }
    else { total = (total + section + Number(current || '0')) * unit; section = 0; }
    current = '';
  }
  const result = total + section + Number(current || '0');
  return Number.isSafeInteger(result) ? result : null;
}

export function spokenNumbers(source) {
  const text = String(source).normalize('NFKC').replace(/负(?=[零〇一幺二两三四五六七八九十百千万亿0-9])/g, '-')
    .replace(/[零〇一幺二两三四五六七八九十百千万亿0-9]+(?:[点\.][零〇一幺二两三四五六七八九0-9]+)?/g, token => {
      const [whole, fraction] = token.split(/[点\.]/);
      const value = chineseNumber(whole);
      if (value === null) return token;
      return String(value) + (fraction === undefined ? '' : '.' + [...fraction].map(character => digits[character] ?? character).join(''));
    });
  return englishSpokenNumbers(text);
}

export function dictationNumber(source) {
  const text = spokenNumbers(source).replace(/[。！!？?，,、；;：:\s]/g, '').replace(/[^0-9+\-.]/g, '').replace(/\.$/, '');
  if (!/^[-+]?(?:\d+(?:\.\d*)?|\.\d+)$/.test(text)) return null;
  const value = Number(text);
  return Number.isFinite(value) ? String(value) : null;
}

export function matchesDictationKey(event, shortcut = 'TILDE') {
  const parsed = parseShortcut(shortcut);
  if (parsed.error || !parsed.key || Boolean(event.ctrlKey || event.metaKey) !== parsed.parts.includes('LEFTCTRL') || Boolean(event.altKey) !== parsed.parts.includes('LEFTALT')) return false;
  if (parsed.key === 'TILDE' && event.code === 'Backquote') return !event.ctrlKey && !event.metaKey && !event.altKey && (!parsed.parts.includes('LEFTSHIFT') || event.shiftKey);
  return Boolean(event.shiftKey) === parsed.parts.includes('LEFTSHIFT') && (event.metaKey ? parsed.parts.includes('LEFTCTRL') : true) && shortcutKey(event) === parsed.key;
}

export function editableDictationTarget(event) {
  const path = event.composedPath?.() ?? [event.target];
  for (const node of path) {
    if (!node?.tagName || node.closest?.('[inert]') || node.disabled || node.readOnly || node.dataset?.dictation === 'off') continue;
    if (node.tagName === 'TEXTAREA') return node;
    if (node.tagName === 'INPUT' && ['text', 'search', 'url', 'email', 'tel', 'number', 'password'].includes(node.type)) return node;
    if (node.isContentEditable) return node;
  }
  return null;
}

export function dictationFieldKind(input) {
  if (input.dataset?.numericExpression === 'beat') return 'beat';
  return input.type === 'number' || input.classList?.contains('numeric-expression-number') || input.dataset?.numericExpression === 'number' ? 'number' : 'text';
}

export function prepareDictationText(source, kind = 'text') {
  const text = String(source).replace(/<\|[^|]*\|>/g, '').trim();
  if (!text) return '';
  if (kind === 'number') return dictationNumber(text);
  if (kind === 'beat') return spokenNumbers(text).replace(/\bcolon\b/gi, ':').replace(/\b(?:slash|over)\b/gi, '/').replace(/[。！!？?，,、；;\s]/g, '').replaceAll('冒号', ':').replaceAll('斜杠', '/');
  return englishSpokenNumbers(text.replace(/(?<![\p{Script=Han}])负?[零〇一幺二两三四五六七八九十百千万亿]+(?:点[零〇一幺二两三四五六七八九]+)?(?![\p{Script=Han}])/gu, spokenNumbers));
}

export function validDictationPcm(value, maximum = 32000) {
  return value instanceof Float32Array && value.length > 0 && value.length <= maximum && value.every(sample => Number.isFinite(sample) && Math.abs(sample) <= 1.01);
}
