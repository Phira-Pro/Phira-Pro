import { t } from '../i18n.mjs';
export function agentWorkspacePath(value, { root = false } = {}) {
  if (root && (value === '' || value === undefined)) return '';
  if (typeof value !== 'string' || !value || value.length > 1024 || /[\\:\x00-\x1f]/.test(value) || value.startsWith('/') || value.split('/').some(part => !part || part === '.' || part === '..' || /[. ]$/.test(part) || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(part))) throw Error(t('agent-files.s6344f78'));
  return value;
}

export function agentDownloadUrl(value) {
  const url = new URL(value);
  if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || /^(localhost|127\.|0\.|10\.|192\.168\.|169\.254\.|172\.(1[6-9]|2\d|3[01])\.|\[|.*\.local$)/i.test(url.hostname)) throw Error(t('agent-files.sb82e5cc'));
  return url.href;
}

export const agentExecutableFile = path => /\.(?:exe|dll|com|scr|msi|bat|cmd|ps1|sh|py|js|mjs|cjs|vbs|app|rpmod)$/i.test(path);
