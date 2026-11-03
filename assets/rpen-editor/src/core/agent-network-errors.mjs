import { t } from '../i18n.mjs';

export function redactAgentError(value, key = '') {
  let text = String(value ?? ''); if (key) text = text.replaceAll(key, '[已隐藏]');
  return text.replace(/Bearer\s+[^\s"'<>]+/gi, 'Bearer [已隐藏]');
}
const parse = value => { if (typeof value !== 'string') return value; try { return JSON.parse(value); } catch { return value; } };
const messageOf = value => typeof value === 'string' ? value : value?.error?.message ?? value?.message ?? (typeof value?.error === 'string' ? value.error : '');
const endpoint = url => { try { return new URL(url).host; } catch { return 'API'; } };

export function agentApiError({ url, status, statusText, body, key, requestId } = {}) {
  const payload = parse(body), error = payload?.error ?? payload, metadata = error?.metadata;
  const upstream = messageOf(parse(metadata?.raw)), primary = messageOf(payload), html = typeof body === 'string' && /^\s*(?:<!doctype|<html|<head|<body)/i.test(body);
  const fallback = ({ 400: t('agent-provider.s6edbadf'), 401: t('agent-provider.sf3a6b8a'), 402: t('agent-provider.sc1ab622'), 403: t('agent-provider.sc96b373'), 429: t('agent-provider.s14c9516'), 503: t('agent-provider.s4cf3d3f') })[status] ?? (statusText || t('agent-provider.s2b93754'));
  const parts = html ? [t('agent-network.htmlResponse')] : [primary || fallback, upstream && upstream !== primary ? upstream : ''];
  const code = error?.code ?? error?.type, provider = metadata?.provider_name;
  if (code && String(code) !== String(status)) parts.push('[' + code + ']');
  if (provider) parts.push('(' + provider + ')');
  const detail = redactAgentError(parts.filter(Boolean).join(' · '), key).replace(/\s+/g, ' ').slice(0, 1500);
  const message = endpoint(url) + (status ? ' HTTP ' + status : '') + ': ' + detail;
  return Object.assign(Error(message), { status, code, responseBody: redactAgentError(typeof body === 'string' ? body : JSON.stringify(body ?? {}), key), requestId: redactAgentError(requestId ?? error?.request_id ?? payload?.request_id ?? '', key), endpoint: url });
}

export function agentConnectionError(error, { url, key, timedOut = false, browser = false } = {}) {
  const detail = redactAgentError(error?.message || String(error), key).replace(/\s+/g, ' ').slice(0, 500);
  const message = t(timedOut ? 'agent-network.timeout' : browser ? 'agent-network.browserConnection' : 'agent-network.connection', { host: endpoint(url), detail });
  return Object.assign(Error(message), { code: error?.code, endpoint: url });
}

export async function agentResponseText(response, limit = 16 * 1024 * 1024) {
  if (!response.body) return '';
  const reader = response.body.getReader(), decoder = new TextDecoder(); let size = 0, text = '';
  try {
    for (;;) { const { done, value } = await reader.read(); if (done) break; size += value.byteLength; if (size > limit) throw Error(t('agent-provider.s4bd8619')); text += decoder.decode(value, { stream: true }); }
    return text + decoder.decode();
  } finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
}
