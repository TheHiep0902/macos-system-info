<script lang="ts">
  import { clusterName, formatMhz, formatPercent } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import { statusFor, statusLabel } from '../../lib/thresholds';
  import type { ClusterInfo, ClusterSample, FrequencySample, MetricState } from '../../lib/types';
  import ProgressBar from '../primitives/ProgressBar.svelte';
  import Skeleton from '../primitives/Skeleton.svelte';

  interface Props {
    clusters: readonly ClusterSample[];
    info: readonly ClusterInfo[];
    frequency: MetricState<FrequencySample>;
  }

  let { clusters, info, frequency }: Props = $props();

  const rows = $derived(
    clusters.map((cluster) => {
      const name = clusterName(cluster.label, info);
      return {
        label: cluster.label,
        name,
        usagePct: cluster.usagePct,
        freqMhz: cluster.freqMhz,
        maxMhz: info.find((entry) => entry.label === cluster.label)?.maxMhz ?? null,
        status: statusFor('cpu', cluster.usagePct),
      };
    }),
  );
</script>

<div class="summary">
  {#if frequency.state === 'unavailable'}
    <p class="metric-reason">{frequency.reason}</p>
  {/if}
  {#if rows.length === 0}
    <Skeleton lines={2} />
  {:else}
    <ul class="clusters">
      {#each rows as row (row.label)}
        <li class="cluster">
          <div class="row">
            <span class="name">{row.name}</span>
            <span class="usage">{formatPercent(row.usagePct)}</span>
          </div>
          <ProgressBar
            value={row.usagePct}
            size="sm"
            label={t('detail.coreUsageLabel', { name: row.name })}
            valueText={`${formatPercent(row.usagePct)} · ${statusLabel(row.status)}`}
            status={row.status}
          />
          <div class="row">
            <span class="metric-secondary">
              {#if frequency.state === 'loading'}
                {t('freq.loading')}
              {:else}
                {t('cluster.frequency')}: {formatMhz(row.freqMhz)}
              {/if}
            </span>
            {#if row.maxMhz !== null}
              <span class="metric-note">{t('cluster.max', { mhz: formatMhz(row.maxMhz) })}</span>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .summary {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .clusters {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .cluster {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .name {
    font-weight: var(--font-weight-medium);
  }

  .usage {
    font-weight: var(--font-weight-semibold);
  }
</style>
