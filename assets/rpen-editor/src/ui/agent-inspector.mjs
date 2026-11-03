import { t } from '../i18n.mjs';
const node = (tag, value = '') => { const element = document.createElement(tag); element.textContent = value; return element; };

export function showAgentExchange(turn, read) {
  const dialog = node('dialog'); dialog.className = 'agent-exchange-dialog'; dialog.dataset.agentPrivate = 'true';
  const heading = node('div'); heading.className = 'agent-heading';
  const close = node('button', t('agent-inspector.s6c14bd7')); close.type = 'button'; close.onclick = () => dialog.close(); heading.append(node('h3', t('agent-inspector.s4b0ebb6')), close);
  const select = node('select'); select.setAttribute('aria-label', t('agent-inspector.s708d5bf'));
  for (const [index, trace] of (turn.exchanges ?? []).entries()) { const option = node('option', t('agent-inspector.s24bfd77', { p0: index + 1, p1: trace.purpose === 'compaction' ? t('agent-inspector.sc8798fb') : t('agent-inspector.s890ceb1'), p2: trace.model, p3: trace.status })); option.value = trace.id; select.append(option); }
  const controls = node('div'); controls.className = 'agent-exchange-controls'; const request = node('button', t('agent-inspector.s926f6b6')), response = node('button', t('agent-inspector.s11d0241')), refresh = node('button', t('agent-inspector.s38108ea')), copy = node('button', t('agent-inspector.s4edd1d0'));
  const meta = node('p'); meta.className = 'hint'; const content = node('textarea'); content.readOnly = true; content.spellcheck = false; content.setAttribute('aria-label', t('agent-inspector.sb0f6dd3'));
  let part = 'request', generation = 0;
  const render = async () => {
    const serial = ++generation; content.value = t('agent-inspector.sfcabadb');
    try {
      const trace = await read(select.value); if (serial !== generation) return;
      meta.textContent = trace ? t('agent-inspector.s3d53bd1', { p0: trace.model, p1: trace.status, p2: ((trace.durationMs ?? 0) / 1000).toFixed(2) }) : t('agent-inspector.s098c45f');
      const value = trace?.[part]; content.value = part === 'response' && typeof value?.body === 'string' ? value.body : typeof value === 'string' ? value : JSON.stringify(value ?? (part === 'response' ? t('agent-inspector.s26d09b0') : {}), null, 2);
      request.classList.toggle('active', part === 'request'); response.classList.toggle('active', part === 'response');
    } catch (error) { if (serial === generation) content.value = error.message; }
  };
  request.onclick = () => { part = 'request'; render(); }; response.onclick = () => { part = 'response'; render(); }; refresh.onclick = render; select.onchange = render;
  copy.onclick = async () => { try { await navigator.clipboard.writeText(content.value); meta.textContent = t('agent-inspector.s96ec4af'); } catch { meta.textContent = t('agent-inspector.sb29165b'); } }; controls.append(select, request, response, refresh, copy);
  dialog.append(heading, controls, meta, content); dialog.addEventListener('close', () => { generation++; dialog.remove(); }, { once: true }); document.body.append(dialog); dialog.showModal(); render();
}
