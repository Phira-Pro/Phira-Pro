export class PreviewBackground {
  draw(context, image, width, height, scale, blur, ratio, animated = false) {
    const paint = target => {
      const size = Math.max(1350 / image.naturalWidth, 900 / image.naturalHeight) * scale;
      target.filter = blur > 0 ? `blur(${blur}px)` : 'none';
      target.drawImage(image.source ?? image, (width - image.naturalWidth * size) / 2, (height - image.naturalHeight * size) / 2, image.naturalWidth * size, image.naturalHeight * size);
      target.filter = 'none';
    };
    if (animated || typeof document === 'undefined') { paint(context); return; }
    const key = `${width}:${height}:${scale}:${blur}:${ratio}`;
    if (this.image !== image || this.key !== key) {
      this.canvas ??= document.createElement('canvas');
      this.canvas.width = Math.round(width * ratio); this.canvas.height = Math.round(height * ratio);
      const target = this.canvas.getContext('2d'); target.setTransform(ratio, 0, 0, ratio, 0, 0);
      paint(target); this.image = image; this.key = key;
    }
    context.drawImage(this.canvas, 0, 0, this.canvas.width / ratio, this.canvas.height / ratio);
  }

  clear() { this.image = null; this.canvas = null; this.key = null; }
}

export function textureInViewport(texture, line, state, width, height, scale, viewport) {
  const anchor = line.anchor ?? [0.5, 0.5];
  const left = -texture.naturalWidth * anchor[0] * scale * state.scaleX;
  const right = texture.naturalWidth * (1 - anchor[0]) * scale * state.scaleX;
  const top = -texture.naturalHeight * (1 - anchor[1]) * scale * state.scaleY;
  const bottom = texture.naturalHeight * anchor[1] * scale * state.scaleY;
  const angle = state.rotation * Math.PI / 180; const cosine = Math.cos(angle); const sine = Math.sin(angle);
  const horizontal = width / 2 + state.x * scale; const vertical = height / 2 - state.y * scale;
  const minimumX = horizontal + Math.min(left * cosine, right * cosine) + Math.min(-top * sine, -bottom * sine);
  const maximumX = horizontal + Math.max(left * cosine, right * cosine) + Math.max(-top * sine, -bottom * sine);
  const minimumY = vertical + Math.min(left * sine, right * sine) + Math.min(top * cosine, bottom * cosine);
  const maximumY = vertical + Math.max(left * sine, right * sine) + Math.max(top * cosine, bottom * cosine);
  return maximumX >= viewport.left - 1 && minimumX <= viewport.left + viewport.width + 1
    && maximumY >= viewport.top - 1 && minimumY <= viewport.top + viewport.height + 1;
}
