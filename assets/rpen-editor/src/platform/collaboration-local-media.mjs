const fromHex = value => Uint8Array.from(value.match(/../g), part => parseInt(part, 16));

export async function verifyLocalMediaRoute(base, hint, request, signal) {
  if (!Number.isInteger(hint?.port) || hint.port < 1 || hint.port > 65535 || typeof hint.proof !== 'string' || !/^[0-9a-f]{64}$/.test(hint.proof)) return null;
  const nonce = crypto.randomUUID().replaceAll('-', '');
  const local = new URL(base); local.protocol = 'http:'; local.hostname = '127.0.0.1'; local.port = String(hint.port);
  const response = await request(new URL(`/collab/media/local/${nonce}`, local), {
    method: 'GET', signal: AbortSignal.any([signal, AbortSignal.timeout(1500)]),
    credentials: 'omit', redirect: 'error', referrerPolicy: 'no-referrer', cache: 'no-store'
  });
  if (!response.ok) return null;
  const result = await response.json();
  if (typeof result.signature !== 'string' || !/^[0-9a-f]{64}$/.test(result.signature)) return null;
  const key = await crypto.subtle.importKey('raw', fromHex(hint.proof), { name: 'HMAC', hash: 'SHA-256' }, false, ['verify']);
  const valid = await crypto.subtle.verify('HMAC', key, fromHex(result.signature), new TextEncoder().encode(nonce));
  signal.throwIfAborted();
  return valid ? local : null;
}
