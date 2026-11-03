// Work in an unrotated editor plane. Its bounds follow the viewport rather than
// the judgement-line anchor, so even distant anchors need only a bounded canvas.
export function directPlacementPlane(guide, viewport, pixelsPerUnit) {
  if (!guide || !(pixelsPerUnit > 0) || ![guide.x, guide.y, guide.rotation].every(Number.isFinite)) return null;
  const angle = guide.rotation * Math.PI / 180, cosine = Math.cos(angle), sine = Math.sin(angle);
  const inverse = point => ({ x: (point.x - guide.x) * cosine + (point.y - guide.y) * sine,
    y: -(point.x - guide.x) * sine + (point.y - guide.y) * cosine });
  const corners = [[0, 0], [1, 0], [0, 1], [1, 1]].map(([x, y]) => inverse({ x: viewport.left + viewport.width * x, y: viewport.top + viewport.height * y }));
  const left = Math.min(...corners.map(point => point.x)) - 12, right = Math.max(...corners.map(point => point.x)) + 12;
  const top = Math.min(...corners.map(point => point.y)) - 12, edge = Math.max(...corners.map(point => point.y)) + 12;
  const height = Math.max(1, Math.ceil(edge - top)), width = Math.max(1, Math.ceil(right - left));
  return { width, height, baseline: -top, originX: -left, pixelsPerUnit, angle,
    matrix: [cosine, sine, -sine, cosine, guide.x + left * cosine - top * sine, guide.y + left * sine + top * cosine],
    point: point => { const local = inverse(point); return { x: local.x - left, y: local.y - top }; },
    screen: point => ({ x: guide.x + (point.x + left) * cosine - (point.y + top) * sine, y: guide.y + (point.x + left) * sine + (point.y + top) * cosine }) };
}
