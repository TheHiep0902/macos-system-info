<script lang="ts">
  import { sourceLabel } from '../../lib/format';
  import { t } from '../../lib/i18n';

  interface Props {
    source: string;
  }

  let { source }: Props = $props();

  const label = $derived(sourceLabel(source));
</script>

<span class="source" title={t('source.label', { source: label })}>
  <span class="sr-only">{t('source.label', { source: '' })}</span>{label}
</span>

<style>
  .source {
    display: inline-flex;
    align-items: center;
    padding: 0 var(--space-2);
    border: var(--border-width) solid var(--color-border-subtle);
    border-radius: var(--radius-sm);
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
    line-height: var(--line-height-normal);
    white-space: nowrap;
  }
</style>
