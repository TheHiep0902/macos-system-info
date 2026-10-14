import { describe, expect, it } from 'vitest';
import { resolveEffects, resolveTheme } from './theme';

describe('resolveTheme', () => {
  it('honours an explicit preference regardless of the system', () => {
    expect(resolveTheme('light', true)).toBe('light');
    expect(resolveTheme('dark', false)).toBe('dark');
  });

  it('follows the system when set to system', () => {
    expect(resolveTheme('system', true)).toBe('dark');
    expect(resolveTheme('system', false)).toBe('light');
  });
});

describe('resolveEffects', () => {
  it('reduces effects when the setting or the system asks for it', () => {
    expect(resolveEffects(false, false)).toBe('full');
    expect(resolveEffects(true, false)).toBe('reduced');
    expect(resolveEffects(false, true)).toBe('reduced');
  });
});
