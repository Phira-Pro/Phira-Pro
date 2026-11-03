import { t } from '../i18n.mjs';

export const AGENT_IMAGE_LIMITS = Object.freeze({ count: 4, dataLength: 1536 * 1024, totalLength: 6 * 1024 * 1024, fileBytes: 20 * 1024 * 1024, edge: 1600 });
export function agentImages(images = []) {
  if (!Array.isArray(images) || images.length > AGENT_IMAGE_LIMITS.count) throw Error(t('agent-images.limit'));
  let total = 0;
  return images.map(image => {
    const url = typeof image === 'string' ? image : image?.url;
    if (typeof url !== 'string' || !/^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/]+=*$/.test(url) || url.length > AGENT_IMAGE_LIMITS.dataLength || (total += url.length) > AGENT_IMAGE_LIMITS.totalLength) throw Error(t('agent-images.invalid'));
    return { id: image.id ?? crypto.randomUUID(), name: String(image.name ?? t('agent-images.image')).slice(0, 120), width: image.width, height: image.height, url };
  });
}
export function agentUserMessage(text, images = [], inputId) {
  const attachments = agentImages(images);
  return { role: 'user', content: attachments.length ? [{ type: 'text', text }, ...attachments.map(image => ({ type: 'image_url', image_url: { url: image.url, detail: 'auto' } }))] : text,
    ...(attachments.length ? { rpen_input_images: inputId ?? true } : {}) };
}
export function agentTurnImages(turn) {
  const content = turn.messages?.find(message => message.role === 'user')?.content;
  const parts = Array.isArray(content) ? content.filter(part => part.type === 'image_url') : [];
  return parts.map((part, index) => ({ ...(turn.imageNames?.[index] ?? {}), url: part.image_url.url }));
}
