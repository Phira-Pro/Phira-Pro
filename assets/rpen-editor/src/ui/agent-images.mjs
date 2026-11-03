import { AGENT_IMAGE_LIMITS, agentImages } from '../core/agent-images.mjs';
import { t } from '../i18n.mjs';

const blobUrl = blob => new Promise((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(reader.result); reader.onerror = () => reject(reader.error); reader.readAsDataURL(blob); });
export async function prepareAgentImage(file) {
  if (!/^image\/(png|jpe?g|webp|gif|avif)$/i.test(file.type) || file.size > AGENT_IMAGE_LIMITS.fileBytes) throw Error(t('agent-images.invalid'));
  const image = await createImageBitmap(file);
  try {
    if (!image.width || !image.height) throw Error(t('agent-images.invalid'));
    const scale = Math.min(1, AGENT_IMAGE_LIMITS.edge / Math.max(image.width, image.height));
    const canvas = document.createElement('canvas'); canvas.width = Math.max(1, Math.round(image.width * scale)); canvas.height = Math.max(1, Math.round(image.height * scale));
    const context = canvas.getContext('2d'); context.drawImage(image, 0, 0, canvas.width, canvas.height);
    let blob = await new Promise(resolve => canvas.toBlob(resolve, 'image/png'));
    if (!blob) throw Error(t('agent-images.invalid'));
    if (blob.size * 4 / 3 > AGENT_IMAGE_LIMITS.dataLength - 64) {
      context.globalCompositeOperation = 'destination-over'; context.fillStyle = '#fff'; context.fillRect(0, 0, canvas.width, canvas.height);
      for (const quality of [.88, .7, .5]) { blob = await new Promise(resolve => canvas.toBlob(resolve, 'image/jpeg', quality)); if (blob && blob.size * 4 / 3 < AGENT_IMAGE_LIMITS.dataLength - 64) break; }
    }
    return agentImages([{ id: crypto.randomUUID(), name: file.name || t('agent-images.image'), width: canvas.width, height: canvas.height, url: await blobUrl(blob) }])[0];
  } finally { image.close(); }
}
export function imageCards(host, images, remove) {
  host.replaceChildren(); host.hidden = !images?.length;
  for (const [index, image] of (images ?? []).entries()) {
    if (typeof image?.url !== 'string' || !/^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/]+=*$/.test(image.url)) continue;
    const card = document.createElement('div'); card.className = 'agent-image-card';
    const open = document.createElement('button'); open.type = 'button'; open.className = 'agent-image-open'; open.title = image.name || t('agent-images.image');
    const picture = document.createElement('img'); picture.src = image.url; picture.alt = open.title; open.append(picture);
    open.onclick = () => {
      const dialog = document.createElement('dialog'); dialog.className = 'agent-image-dialog';
      const full = document.createElement('img'); full.src = image.url; full.alt = picture.alt;
      const close = document.createElement('button'); close.type = 'button'; close.textContent = '×'; close.setAttribute('aria-label', t('agent-images.close')); close.onclick = () => dialog.close();
      dialog.append(full, close); dialog.addEventListener('close', () => dialog.remove(), { once: true }); document.body.append(dialog); dialog.showModal();
    };
    card.append(open);
    if (remove) { const button = document.createElement('button'); button.type = 'button'; button.className = 'agent-image-remove'; button.textContent = '×'; button.setAttribute('aria-label', t('agent-images.remove')); button.onclick = () => remove(index); card.append(button); }
    host.append(card);
  }
}
