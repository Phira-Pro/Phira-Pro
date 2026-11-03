export function numericWheel(input, step = 1, update) {
  input.addEventListener('wheel', event => {
    if (!event.deltaY || input.disabled || input.readOnly) return;
    event.preventDefault(); event.stopPropagation();
    const direction = -Math.sign(event.deltaY);
    if (update) { update(direction); return; }
    const value = Number(input.value);
    if (!Number.isFinite(value)) return;
    const minimum = input.min === '' ? -Infinity : Number(input.min);
    const maximum = input.max === '' ? Infinity : Number(input.max);
    input.value = String(Number(Math.max(minimum, Math.min(maximum, value + direction * step)).toFixed(10)));
    input.dispatchEvent(new Event('change', { bubbles: true }));
  }, { passive: false });
}
