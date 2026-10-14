<script lang="ts">
  import { formatMhz, formatPercent } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import type { FrequencySample, MetricState, StaticInfo } from '../../lib/types';
  import Skeleton from '../primitives/Skeleton.svelte';

  interface Props {
    frequency: MetricState<FrequencySample>;
    staticInfo: StaticInfo;
  }

  let { frequency, staticInfo }: Props = $props();
</script>

<div class="panel">
  {#if frequency.state === 'loading'}
    <Skeleton lines={2} label={t('freq.loading')} />
  {:else if frequency.state === 'unavailable'}
    <p class="metric-reason">{frequency.reason}</p>
  {:else}
    {#if frequency.source === 'nominal'}
      <p class="nominal">
        {t('freq.nominal', {
          nominal: formatMhz(staticInfo.nominalMhz),
          max: formatMhz(staticInfo.maxMhz),
          pct: formatPercent(frequency.speedLimitPct),
        })}
      </p>
    {:else}
      <dl class="metric-list">
        <dt>{t('freq.avg')}</dt>
        <dd>{formatMhz(frequency.avgMhz)}</dd>
        <dt>{t('freq.min')}</dt>
        <dd>{formatMhz(frequency.minMhz)}</dd>
        <dt>{t('freq.max')}</dt>
        <dd>{formatMhz(frequency.maxMhz)}</dd>
        {#if frequency.speedLimitPct !== null}
          <dt>{t('freq.speedLimit')}</dt>
          <dd>{formatPercent(frequency.speedLimitPct)}</dd>
        {/if}
      </dl>
    {/if}
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .nominal {
    font-size: var(--font-size-sm);
  }
</style>
