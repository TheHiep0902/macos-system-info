<script lang="ts">
  import type { LucideIcon } from '@lucide/svelte';

  interface Props {
    title: string;
    description?: string;
    icon?: LucideIcon;
    tone?: 'neutral' | 'error';
  }

  let { title, description, icon: Icon, tone = 'neutral' }: Props = $props();
</script>

<div class="empty" data-tone={tone}>
  {#if Icon}
    <Icon class="empty-icon" aria-hidden="true" />
  {/if}
  <div class="text">
    <p class="title">{title}</p>
    {#if description}
      <p class="description">{description}</p>
    {/if}
  </div>
</div>

<style>
  .empty {
    display: flex;
    align-items: flex-start;
    gap: var(--space-4);
    color: var(--color-text-secondary);
  }

  .empty[data-tone='error'] {
    color: var(--color-status-critical);
  }

  .empty :global(.empty-icon) {
    flex: none;
    width: var(--icon-size-md);
    height: var(--icon-size-md);
  }

  .title {
    font-size: var(--font-size-md);
  }

  .description {
    color: var(--color-text-tertiary);
    font-size: var(--font-size-sm);
  }
</style>
