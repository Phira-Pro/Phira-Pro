export function assetBase64(bytes) {
  const parts = [];
  for (let offset = 0; offset < bytes.length; offset += 8192) parts.push(String.fromCharCode(...bytes.subarray(offset, offset + 8192)));
  return btoa(parts.join(''));
}

export async function sendCollaborationAsset(transport, name, bytes, digest, onProgress = () => {}) {
  const chunkSize = transport.assetDelivery ? 256 * 1024 : 49152;
  const total = Math.ceil(bytes.length / chunkSize);
  const parallel = transport.assetDelivery ? 4 : 1;
  const transfer = crypto.randomUUID();
  let next = 0; let completed = 0; let failure;
  const workers = Array.from({ length: Math.min(parallel, total) }, async () => {
    while (!failure && next < total) {
      const index = next++;
      const chunk = bytes.subarray(index * chunkSize, (index + 1) * chunkSize);
      try {
        await transport.sendAsset({ type: 'asset', name, hash: digest, transfer, index, total, data: assetBase64(chunk) });
        completed += chunk.length; onProgress(completed, bytes.length);
      } catch (error) { failure ??= error; }
    }
  });
  await Promise.all(workers);
  if (failure) throw failure;
}
