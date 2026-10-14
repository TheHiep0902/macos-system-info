<script lang="ts">
  import type { LucideIcon } from '@lucide/svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    title: string;
    icon: LucideIcon;
    tone: 'cpu' | 'power' | 'memory' | 'disk' | 'network' | 'uptime';
    busy?: boolean;
    aside?: Snippet;
    children: Snippet;
  }

  let { title, icon: Icon, tone, busy = false, aside, children }: Props = $props();

  const headingId = $props.id();
</script>

<section class="card" data-tone={tone} aria-labelledby={headingId} aria-busy={busy}>
  <div class="card-header">
    <Icon class="card-icon" aria-hidden="true" />
    <h2 class="card-title" id={headingId}>{title}</h2>
    {#if aside}
      <div class="aside">{@render aside()}</div>
    {/if}
  </div>
  {@render children()}
</section>

<style>
  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-5) var(--space-6);
    border: var(--border-width) solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
    background: var(--color-surface-card);
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--icon-size-md);
  }

  .card :global(.card-icon) {
    flex: none;
    width: var(--icon-size-sm);
    height: var(--icon-size-sm);
    color: var(--color-chart-cpu);
  }

  .card[data-tone='power'] :global(.card-icon) {
    color: var(--color-chart-power);
  }

  .card[data-tone='memory'] :global(.card-icon) {
    color: var(--color-chart-memory);
  }

  .card[data-tone='disk'] :global(.card-icon) {
    color: var(--color-chart-disk);
  }

  .card[data-tone='network'] :global(.card-icon) {
    color: var(--color-chart-network);
  }

  .card[data-tone='uptime'] :global(.card-icon) {
    color: var(--color-text-secondary);
  }

  .card-title {
    flex: 1 1 auto;
    min-width: 0;
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-semibold);
    line-height: var(--line-height-tight);
  }

  .aside {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--space-2);
  }
</style>
