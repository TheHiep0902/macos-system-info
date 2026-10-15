import { describe, expect, it } from 'vitest';
import { statusFor, statusLabel } from './thresholds';

describe('statusFor', () => {
  it.each([
    [69.9, 'normal'],
    [70, 'warn'],
    [89.9, 'warn'],
    [90, 'critical'],
    [100, 'critical'],
  ] as const)('CPU %s%% is %s', (value, expected) => {
    expect(statusFor('cpu', value)).toBe(expected);
  });

  it.each([
    [74.9, 'normal'],
    [75, 'warn'],
    [89.9, 'warn'],
    [90, 'critical'],
  ] as const)('memory %s%% is %s', (value, expected) => {
    expect(statusFor('memory', value)).toBe(expected);
  });

  it.each([
    [79.9, 'normal'],
    [80, 'warn'],
    [94.9, 'warn'],
    [95, 'critical'],
  ] as const)('disk %s%% is %s', (value, expected) => {
    expect(statusFor('disk', value)).toBe(expected);
  });

  it.each([
    [79.9, 'normal'],
    [80, 'warn'],
    [94.9, 'warn'],
    [95, 'critical'],
    [110, 'critical'],
  ] as const)('temperature %s °C is %s', (value, expected) => {
    expect(statusFor('temperature', value)).toBe(expected);
  });

  it('treats non-finite values as unavailable', () => {
    expect(statusFor('cpu', Number.NaN)).toBe('unavailable');
  });
});

describe('statusLabel', () => {
  it('gives every status a text label so colour is never the only signal', () => {
    expect(statusLabel('normal')).toBe('Bình thường');
    expect(statusLabel('warn')).toBe('Cao');
    expect(statusLabel('critical')).toBe('Rất cao');
    expect(statusLabel('unavailable')).toBe('Không khả dụng');
  });
});
