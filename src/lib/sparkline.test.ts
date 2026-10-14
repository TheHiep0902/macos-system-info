import { describe, expect, it } from 'vitest';
import { seriesRange, sparklinePoints } from './sparkline';

describe('sparklinePoints', () => {
  it('needs at least two points to draw a line', () => {
    expect(sparklinePoints([], 60, 100)).toBe('');
    expect(sparklinePoints([50], 60, 100)).toBe('');
  });

  it('right-aligns the series and scales against a fixed maximum', () => {
    expect(sparklinePoints([0, 50, 100], 4, 100)).toBe('1,96 2,50 3,4');
  });

  it('clamps out-of-range values and drops non-finite ones', () => {
    expect(sparklinePoints([-10, Number.NaN, 150], 2, 100)).toBe('0,96 1,4');
  });

  it('auto-scales with headroom when no maximum is given', () => {
    expect(sparklinePoints([5, 10], 2, null)).toBe('0,57.7 1,19.3');
  });

  it('keeps only the newest points that fit', () => {
    expect(sparklinePoints([10, 20, 30, 40], 2, 100).split(' ')).toHaveLength(2);
  });
});

describe('seriesRange', () => {
  it('returns the minimum and maximum of finite values', () => {
    expect(seriesRange([3, Number.NaN, 9, 1])).toEqual({ min: 1, max: 9 });
    expect(seriesRange([])).toBeNull();
  });
});
