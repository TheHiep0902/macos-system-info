import type { ThemePreference } from './types';

export type ResolvedTheme = 'light' | 'dark';

export type EffectsMode = 'full' | 'reduced';

export const DARK_SCHEME_QUERY = '(prefers-color-scheme: dark)';

export const REDUCED_TRANSPARENCY_QUERY = '(prefers-reduced-transparency: reduce)';

export function resolveTheme(
  preference: ThemePreference,
  systemPrefersDark: boolean,
): ResolvedTheme {
  if (preference === 'light' || preference === 'dark') {
    return preference;
  }
  return systemPrefersDark ? 'dark' : 'light';
}

export function resolveEffects(
  reduceEffectsSetting: boolean,
  systemPrefersReducedTransparency: boolean,
): EffectsMode {
  return reduceEffectsSetting || systemPrefersReducedTransparency ? 'reduced' : 'full';
}
