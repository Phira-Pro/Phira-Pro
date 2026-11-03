import { t } from '../i18n.mjs';
export function agentLink(value) {
  try { const url = new URL(value); return ['https:', 'http:', 'mailto:'].includes(url.protocol) ? url.href : null; } catch { return null; }
}

function inline(parent, source, depth = 0) {
  if (depth > 8) { parent.append(document.createTextNode(source)); return; }
  const pattern = /\\([\\`*_{}\[\]()#+.!>|~-])|`([^`\n]+)`|(!?)\[([^\]\n]+)\]\(([^\s)]+)(?:\s+"[^"]*")?\)|\*\*([^*]+)\*\*|__([^_]+)__|~~([^~]+)~~|\*([^*\n]+)\*|_([^_\n]+)_/g;
  let offset = 0;
  for (const match of source.matchAll(pattern)) {
    parent.append(document.createTextNode(source.slice(offset, match.index))); offset = match.index + match[0].length;
    if (match[1]) { parent.append(document.createTextNode(match[1])); continue; }
    if (match[2]) { const code = document.createElement('code'); code.textContent = match[2]; parent.append(code); continue; }
    if (match[4]) {
      const href = agentLink(match[5]);
      if (!href) { parent.append(document.createTextNode(match[4])); continue; }
      const link = document.createElement('a'); link.href = href; link.target = '_blank'; link.rel = 'noopener noreferrer'; inline(link, match[4], depth + 1); parent.append(link); continue;
    }
    const node = document.createElement(match[6] || match[7] ? 'strong' : match[8] ? 'del' : 'em');
    inline(node, match[6] ?? match[7] ?? match[8] ?? match[9] ?? match[10], depth + 1); parent.append(node);
  }
  parent.append(document.createTextNode(source.slice(offset)));
}

export function renderAgentMarkdown(target, source) {
  target.dataset.markdownSource = String(source ?? '');
  const root = document.createDocumentFragment(), lines = String(source ?? '').replaceAll('\r\n', '\n').split('\n');
  const renderInline = (tag, value, parent = root) => { const node = document.createElement(tag); inline(node, value); parent.append(node); return node; };
  const cells = line => line.trim().replace(/^\||\|$/g, '').split(/(?<!\\)\|/).map(cell => cell.trim().replaceAll('\\|', '|'));
  for (let index = 0; index < lines.length;) {
    const line = lines[index];
    if (!line.trim()) { index++; continue; }
    const fence = line.match(/^\s*(`{3,}|~{3,})([^\s]*)/);
    if (fence) {
      const codeLines = []; index++;
      while (index < lines.length && !new RegExp('^\\s*' + fence[1][0] + '{' + fence[1].length + ',}\\s*$').test(lines[index])) codeLines.push(lines[index++]);
      if (index < lines.length) index++;
      const block = document.createElement('div'); block.className = 'agent-code';
      const heading = document.createElement('div'); heading.className = 'agent-code-heading'; const language = document.createElement('small'); language.textContent = fence[2] || t('agent-markdown.s33246dd');
      const copy = document.createElement('button'); copy.type = 'button'; copy.textContent = t('agent-markdown.s4edd1d0'); copy.onclick = async () => { try { await navigator.clipboard.writeText(codeLines.join('\n')); copy.textContent = t('agent-markdown.se381a57'); } catch { copy.textContent = t('agent-markdown.seb6f690'); } };
      const pre = document.createElement('pre'), code = document.createElement('code'); code.textContent = codeLines.join('\n'); pre.append(code); heading.append(language, copy); block.append(heading, pre); root.append(block); continue;
    }
    const heading = line.match(/^\s{0,3}(#{1,6})\s+(.+?)\s*#*$/);
    if (heading) { renderInline('h' + heading[1].length, heading[2]); index++; continue; }
    if (/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line)) { root.append(document.createElement('hr')); index++; continue; }
    if (index + 1 < lines.length && line.includes('|') && /^\s*\|?\s*:?-{3,}:?\s*(?:\|\s*:?-{3,}:?\s*)+\|?\s*$/.test(lines[index + 1])) {
      const container = document.createElement('div'); container.className = 'agent-table'; const table = document.createElement('table'), head = document.createElement('thead'), row = document.createElement('tr');
      for (const cell of cells(line)) renderInline('th', cell, row); head.append(row); table.append(head); index += 2;
      const body = document.createElement('tbody');
      while (index < lines.length && lines[index].trim() && lines[index].includes('|')) { const row = document.createElement('tr'); for (const cell of cells(lines[index++])) renderInline('td', cell, row); body.append(row); }
      table.append(body); container.append(table); root.append(container); continue;
    }
    if (/^\s*>/.test(line)) {
      const quote = document.createElement('blockquote'), content = [];
      while (index < lines.length && /^\s*>/.test(lines[index])) content.push(lines[index++].replace(/^\s*>\s?/, ''));
      renderAgentMarkdown(quote, content.join('\n')); root.append(quote); continue;
    }
    const list = line.match(/^(\s*)([-+*]|\d+[.)])\s+(.+)$/);
    if (list) {
      const ordered = /^\d/.test(list[2]), listNode = document.createElement(ordered ? 'ol' : 'ul'); if (ordered) listNode.start = Number.parseInt(list[2]);
      const base = list[1].length;
      while (index < lines.length) {
        const item = lines[index].match(/^(\s*)([-+*]|\d+[.)])\s+(.+)$/);
        if (!item || item[1].length !== base || /^\d/.test(item[2]) !== ordered) break;
        const entry = renderInline('li', item[3], listNode); index++;
        const nested = [];
        while (index < lines.length && lines[index].trim() && (lines[index].match(/^\s*/)[0].length > base)) nested.push(lines[index++].slice(base + 2));
        if (nested.length) { const child = document.createElement('div'); renderAgentMarkdown(child, nested.join('\n')); entry.append(child); }
      }
      root.append(listNode); continue;
    }
    const paragraph = [line]; index++;
    while (index < lines.length && lines[index].trim() && !/^\s*(?:#{1,6}\s|>|`{3,}|~{3,}|[-+*]\s|\d+[.)]\s)/.test(lines[index]) && !(lines[index].includes('|') && /^\s*\|?\s*:?-{3,}/.test(lines[index + 1] ?? ''))) paragraph.push(lines[index++]);
    const node = document.createElement('p'); paragraph.forEach((entry, row) => { if (row) node.append(document.createElement('br')); inline(node, entry); }); root.append(node);
  }
  target.replaceChildren(root); target.classList.add('agent-markdown');
}
