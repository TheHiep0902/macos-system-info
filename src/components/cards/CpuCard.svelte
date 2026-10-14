<script lang="ts">
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import Cpu from '@lucide/svelte/icons/cpu';
  import { clusterName, formatMhz, formatPercent } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import { seriesRange } from '../../lib/sparkline';
  import { statusFor } from '../../lib/thresholds';
  import type { Snapshot, StaticInfo } from '../../lib/types';
  import ProgressBar from '../primitives/ProgressBar.svelte';
  import Skeleton from '../primitives/Skeleton.svelte';
  import SourceBadge from '../primitives/SourceBadge.svelte';
  import Sparkline from '../primitives/Sparkline.svelte';
  import StatusLabel from '../primitives/StatusLabel.svelte';
  import MetricCard from './MetricCard.svelte';

  interface Props {
    snapshot: Snapshot | null;
    staticInfo: StaticInfo | null;
    history: readonly number[];
    onopen: () => void;
    buttonRef?: HTMLButtonElement | null;
  }

  let { snapshot, staticInfo, history, onopen, buttonRef = $bindable(null) }: Props = $props();

  const total = $derived(snapshot?.cpu.totalUsagePct ?? 0);
  const status = $derived(statusFor('cpu', total));
  const frequency = $derived(snapshot?.cpu.frequency ?? null);
  const clusterFrequencies = $derived(
    (snapshot?.cpu.clusters ?? []).filter((cluster) => cluster.freqMhz !== null),
  );
  const range = $derived(seriesRange(history));
</script>

<MetricCard title={t('cpu.title')} icon={Cpu} tone="cpu" busy={snapshot === null}>
  {#snippet aside()}
    <button
      bind:this={buttonRef}
      type="button"
      class="open"
      aria-label={t('cpu.openDetailLabel')}
      onclick={onopen}
    >
      {t('cpu.openDetail')}
      <ChevronRight class="open-icon" aria-hidden="true" />
    </button>
  {/snippet}

  {#if snapshot === null}
    <Skeleton lines={3} />
  {:else}
    <div class="metric-headline">
      <span class="metric-value">{formatPercent(total)}</span>
      <StatusLabel {status} />
    </div>
    <ProgressBar
      value={total}
      label={t('cpu.usageLabel')}
      valueText={formatPercent(total)}
      {status}
    />
    {#if frequency === null || frequency.state === 'loading'}
      <p class="metric-secondary">{t('cpu.frequencyLoading')}</p>
    {:else if frequency.state === 'unavailable'}
      <p class="metric-reason">{frequency.reason}</p>
    {:else}
      <div class="frequency">
        <span class="metric-secondary">
          {#if frequency.avgMhz !== null}
            {t('cpu.average', { mhz: formatMhz(frequency.avgMhz) })}
          {:else}
            {t('cpu.nominal', { mhz: formatMhz(staticInfo?.nominalMhz) })}
          {/if}
        </span>
        <SourceBadge source={frequency.source} />
      </div>
      {#if clusterFrequencies.length > 1}
        <ul class="clusters">
          {#each clusterFrequencies as cluster (cluster.label)}
            <li class="metric-note">
              {t('cpu.cluster', {
                label: clusterName(cluster.label, staticInfo?.clusters),
                mhz: formatMhz(cluster.freqMhz),
              })}
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
    <Sparkline values={history} tone="cpu" max={100} />
    <p class="sr-only">
      {range === null || history.length < 2
        ? t('chart.collecting')
        : t('chart.summary', { min: formatPercent(range.min), max: formatPercent(range.max) })}
    </p>
  {/if}
</MetricCard>

<style>
  .open {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2) var(--space-1) var(--space-3);
    border: 0;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--color-accent);
    font-size: var(--font-size-sm);
    transition: background-color var(--duration-fast) var(--easing-standard);
  }

  .open::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: var(--radius-lg);
  }

  .open:hover,
  .open:focus-visible {
    background: var(--color-surface-hover);
  }

  .open:active {
    background: var(--color-surface-active);
  }

  .open :global(.open-icon) {
    width: var(--icon-size-sm);
    height: var(--icon-size-sm);
  }

  .frequency {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-4);
  }

  .clusters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-6);
  }
</style>
