<script lang="ts">
  import { formatMhz, formatPercent } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import { statusFor, statusLabel } from '../../lib/thresholds';
  import type { CoreSample } from '../../lib/types';
  import ProgressBar from '../primitives/ProgressBar.svelte';

  interface Props {
    core: CoreSample;
    label: string;
    showMhz: boolean;
  }

  let { core, label, showMhz }: Props = $props();

  const status = $derived(statusFor('cpu', core.usagePct));
</script>

<div class="cell">
  <div class="top">
    <span class="label">{label}</span>
    <span class="usage">{formatPercent(core.usagePct)}</span>
  </div>
  <ProgressBar
    value={core.usagePct}
    size="sm"
    label={t('detail.coreUsageLabel', { name: label })}
    valueText={`${formatPercent(core.usagePct)} · ${statusLabel(status)}`}
    {status}
  />
  {#if showMhz}
    <span class="mhz">{formatMhz(core.freqMhz)}</span>
  {/if}
</div>

<style>
  .cell {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    padding: var(--space-3);
    border: var(--border-width) solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    background: var(--color-surface-card);
  }

  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    column-gap: var(--space-2);
  }

  .label {
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
    white-space: nowrap;
  }

  .usage {
    margin-left: auto;
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-semibold);
    white-space: nowrap;
  }

  .mhz {
    color: var(--color-text-secondary);
    font-size: var(--font-size-xs);
    white-space: nowrap;
  }
</style>
