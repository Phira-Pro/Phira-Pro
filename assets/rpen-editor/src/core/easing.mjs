const halfPi = Math.PI / 2;
const back = 1.70158;
const backInOut = back * 1.525;
const elastic = 2 * Math.PI / 3;
const elasticInOut = 2 * Math.PI / 4.5;

function bounceOut(progress) {
  if (progress < 1 / 2.75) return 7.5625 * progress ** 2;
  if (progress < 2 / 2.75) return 7.5625 * (progress - 1.5 / 2.75) ** 2 + 0.75;
  if (progress < 2.5 / 2.75) return 7.5625 * (progress - 2.25 / 2.75) ** 2 + 0.9375;
  return 7.5625 * (progress - 2.625 / 2.75) ** 2 + 0.984375;
}

const curves = [
  progress => progress,
  progress => Math.sin(progress * halfPi),
  progress => 1 - Math.cos(progress * halfPi),
  progress => 1 - (1 - progress) ** 2,
  progress => progress ** 2,
  progress => (1 - Math.cos(Math.PI * progress)) / 2,
  progress => progress < 0.5 ? 2 * progress ** 2 : 1 - (-2 * progress + 2) ** 2 / 2,
  progress => 1 - (1 - progress) ** 3,
  progress => progress ** 3,
  progress => 1 - (1 - progress) ** 4,
  progress => progress ** 4,
  progress => progress < 0.5 ? 4 * progress ** 3 : 1 - (-2 * progress + 2) ** 3 / 2,
  progress => progress < 0.5 ? 8 * progress ** 4 : 1 - (-2 * progress + 2) ** 4 / 2,
  progress => 1 - (1 - progress) ** 5,
  progress => progress ** 5,
  progress => progress === 1 ? 1 : 1 - 2 ** (-10 * progress),
  progress => progress === 0 ? 0 : 2 ** (10 * progress - 10),
  progress => Math.sqrt(1 - (progress - 1) ** 2),
  progress => 1 - Math.sqrt(Math.abs(1 - progress ** 2)),
  progress => 1 + (back + 1) * (progress - 1) ** 3 + back * (progress - 1) ** 2,
  progress => (back + 1) * progress ** 3 - back * progress ** 2,
  progress => progress < 0.5 ? (1 - Math.sqrt(1 - (2 * progress) ** 2)) / 2 : (Math.sqrt(1 - (-2 * progress + 2) ** 2) + 1) / 2,
  progress => progress < 0.5 ? (2 * progress) ** 2 * ((backInOut + 1) * 2 * progress - backInOut) / 2 : ((2 * progress - 2) ** 2 * ((backInOut + 1) * (2 * progress - 2) + backInOut) + 2) / 2,
  progress => progress === 0 || progress === 1 ? progress : 2 ** (-10 * progress) * Math.sin((progress * 10 - 0.75) * elastic) + 1,
  progress => progress === 0 || progress === 1 ? progress : -(2 ** (10 * progress - 10)) * Math.sin((progress * 10 - 10.75) * elastic),
  bounceOut,
  progress => 1 - bounceOut(1 - progress),
  progress => progress < 0.5 ? (1 - bounceOut(1 - 2 * progress)) / 2 : (1 + bounceOut(2 * progress - 1)) / 2,
  progress => progress === 0 || progress === 1 ? progress : progress < 0.5 ? -(2 ** (20 * progress - 10) * Math.sin((20 * progress - 11.125) * elasticInOut)) / 2 : 2 ** (-20 * progress + 10) * Math.sin((20 * progress - 11.125) * elasticInOut) / 2 + 1,
];

export function easing(progress, type = 1, left = 0, right = 1) {
  const curve = curves[type - 1] ?? curves[0];
  const base = curve(left);
  const range = curve(right) - base;
  if (Math.abs(range) < 1e-12) return progress;
  return (curve(left + (right - left) * progress) - base) / range;
}

export function bezier(progress, points) {
  if (!Array.isArray(points) || points.length !== 4 || !points.every(Number.isFinite)) return progress;
  if (progress <= 0) return 0;
  if (progress >= 1) return 1;
  const [firstX, firstY, secondX, secondY] = points;
  const component = (parameter, first, second) => 3 * parameter * (1 - parameter) ** 2 * first + 3 * parameter ** 2 * (1 - parameter) * second + parameter ** 3;
  let parameter;
  if (firstX === 0 && secondX === 0) parameter = Math.cbrt(progress);
  else if (firstX === 1 && secondX === 1) parameter = 1 - Math.cbrt(1 - progress);
  else {
    const reverse = progress > 0.5, target = reverse ? 1 - progress : progress;
    const first = reverse ? 1 - secondX : firstX, second = reverse ? 1 - firstX : secondX;
    let low = 0, high = 1;
    for (let iteration = 0; iteration < 40; iteration++) {
      const middle = (low + high) / 2;
      if (component(middle, first, second) < target) low = middle;
      else high = middle;
    }
    parameter = reverse ? 1 - (low + high) / 2 : (low + high) / 2;
  }
  return parameter > 0.5 ? 1 - component(1 - parameter, 1 - secondY, 1 - firstY) : component(parameter, firstY, secondY);
}
