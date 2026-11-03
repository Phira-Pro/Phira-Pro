import { assetBytes, resourceReferences, mediaType } from './files.mjs';

const thumbnails = new Map();
export async function projectThumbnail(project) {
  if (typeof document === 'undefined' && typeof OffscreenCanvas === 'undefined') return null;
  const assets = new Map(project.assets);
  for (const [name, value] of assets) if (value instanceof Blob && /(?:^|\/)info\.txt$/i.test(name)) assets.set(name, new Uint8Array(await value.arrayBuffer()));
  const reference = resourceReferences(project.chart, assets, project.chartName, project.info).background;
  const bytes = assetBytes(assets, reference, project.chartName); if (!bytes) return null;
  let key;
  if (globalThis.crypto?.subtle) {
    const digest = await crypto.subtle.digest('SHA-256', bytes instanceof Blob ? await bytes.arrayBuffer() : bytes);
    key = [...new Uint8Array(digest)].map(value => value.toString(16).padStart(2, '0')).join('');
    if (thumbnails.has(key)) { const saved = thumbnails.get(key); thumbnails.delete(key); thumbnails.set(key, saved); return saved; }
  }
  const result = renderThumbnail(bytes, reference);
  if (key) {
    thumbnails.set(key, result);
    while (thumbnails.size > 12) thumbnails.delete(thumbnails.keys().next().value);
    result.then(value => { if (!value && thumbnails.get(key) === result) thumbnails.delete(key); }, () => { if (thumbnails.get(key) === result) thumbnails.delete(key); });
  }
  return result;
}
async function renderThumbnail(bytes, reference) {
  const blob = new Blob([bytes], { type: mediaType(reference) });
  let url; let image;
  try {
    if (typeof document === 'undefined') image = await createImageBitmap(blob);
    else { url = URL.createObjectURL(blob); image = new Image(); image.src = url; await image.decode(); }
    const canvas = typeof document === 'undefined' ? new OffscreenCanvas(480, 270) : document.createElement('canvas'); canvas.width = 480; canvas.height = 270;
    const width = image.naturalWidth ?? image.width; const height = image.naturalHeight ?? image.height;
    const ratio = Math.max(canvas.width / width, canvas.height / height);
    canvas.getContext('2d').drawImage(image, (480 - width * ratio) / 2, (270 - height * ratio) / 2, width * ratio, height * ratio);
    if (canvas.convertToBlob) return await canvas.convertToBlob({ type: 'image/jpeg', quality: 0.8 });
    return await new Promise(resolve => canvas.toBlob(resolve, 'image/jpeg', 0.8));
  } catch { return null; } finally { if (url) URL.revokeObjectURL(url); image?.close?.(); }
}
