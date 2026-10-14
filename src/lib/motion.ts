import type { TransitionConfig } from 'svelte/transition';

export function tokenDurationMs(node: Element, token: string): number {
  const raw = getComputedStyle(node).getPropertyValue(token).trim();
  const value = Number.parseFloat(raw);
  if (!Number.isFinite(value) || value <= 0) {
    return 0;
  }
  if (raw.endsWith('ms')) {
    return value;
  }
  return raw.endsWith('s') ? value * 1000 : value;
}

export function fadeToken(node: Element): TransitionConfig {
  return {
    duration: tokenDurationMs(node, '--duration-fast'),
    css: (progress) => `opacity: ${progress}`,
  };
}
