export const SPARKLINE_HEIGHT = 100;

const VERTICAL_PADDING = 4;
const AUTO_SCALE_HEADROOM = 1.2;
const MIN_AUTO_SCALE = 1;

export function sparklinePoints(
  values: readonly number[],
  capacity: number,
  fixedMax: number | null,
): string {
  const visible = values.filter((value) => Number.isFinite(value)).slice(-capacity);
  if (visible.length < 2) {
    return '';
  }
  const upper =
    fixedMax !== null && fixedMax > 0
      ? fixedMax
      : Math.max(MIN_AUTO_SCALE, Math.max(...visible) * AUTO_SCALE_HEADROOM);
  const usable = SPARKLINE_HEIGHT - VERTICAL_PADDING * 2;
  const offset = capacity - visible.length;
  return visible
    .map((value, index) => {
      const ratio = Math.min(1, Math.max(0, value / upper));
      const x = offset + index;
      const y = SPARKLINE_HEIGHT - VERTICAL_PADDING - ratio * usable;
      return `${x},${Math.round(y * 10) / 10}`;
    })
    .join(' ');
}

export function seriesRange(values: readonly number[]): { min: number; max: number } | null {
  const finite = values.filter((value) => Number.isFinite(value));
  if (finite.length === 0) {
    return null;
  }
  return { min: Math.min(...finite), max: Math.max(...finite) };
}
