export function decodeLegacy(bytes) {
  try { return new TextDecoder('utf-8', { fatal: true }).decode(bytes); }
  catch { return new TextDecoder('gb18030').decode(bytes); }
}

export function parseInfo(text) {
  const info = {};
  for (const line of text.split(/\r?\n/)) {
    const separator = line.indexOf(':');
    if (separator > 0) info[line.slice(0, separator).trim()] = line.slice(separator + 1).trim();
  }
  return info;
}
