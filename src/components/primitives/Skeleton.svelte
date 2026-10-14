<script lang="ts">
  import { t } from '../../lib/i18n';

  interface Props {
    lines?: number;
    label?: string;
  }

  let { lines = 2, label = t('common.loading') }: Props = $props();

  const rows = $derived(Array.from({ length: Math.max(1, lines) }, (_, index) => index));
</script>

<div class="skeleton">
  <span class="sr-only">{label}</span>
  {#each rows as row (row)}
    <span class="line" aria-hidden="true"></span>
  {/each}
</div>

<style>
  .skeleton {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .line {
    display: block;
    height: var(--skeleton-line-height);
    border-radius: var(--radius-sm);
    background: var(--color-surface-skeleton);
  }

  .line:nth-of-type(3n + 2) {
    width: 72%;
  }

  .line:nth-of-type(3n) {
    width: 86%;
  }
</style>
