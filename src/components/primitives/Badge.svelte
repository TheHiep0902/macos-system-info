<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    tone?: 'neutral' | 'accent' | 'warn' | 'critical';
    children: Snippet;
  }

  let { tone = 'neutral', children }: Props = $props();
</script>

<span class="badge" data-tone={tone}>{@render children()}</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-3);
    border-radius: var(--radius-pill);
    background: var(--color-surface-badge);
    color: var(--color-text-secondary);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    line-height: var(--line-height-tight);
    white-space: nowrap;
  }

  .badge[data-tone='accent'] {
    background: var(--color-surface-accent);
    color: var(--color-text-badge-accent);
  }

  .badge[data-tone='warn'] {
    background: var(--color-surface-warn);
    color: var(--color-text-badge-warn);
  }

  .badge[data-tone='critical'] {
    background: var(--color-surface-critical);
    color: var(--color-text-badge-critical);
  }
</style>
