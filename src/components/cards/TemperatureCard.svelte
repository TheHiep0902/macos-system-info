<script lang="ts">
  import Thermometer from '@lucide/svelte/icons/thermometer';
  import { formatCelsius } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import { seriesRange } from '../../lib/sparkline';
  import { statusFor } from '../../lib/thresholds';
  import type { MetricState, TemperatureSample } from '../../lib/types';
  import Skeleton from '../primitives/Skeleton.svelte';
  import SourceBadge from '../primitives/SourceBadge.svelte';
  import Sparkline from '../primitives/Sparkline.svelte';
  import StatusLabel from '../primitives/StatusLabel.svelte';
  import MetricCard from './MetricCard.svelte';

  interface Props {
    temperature: MetricState<TemperatureSample> | null;
    history: readonly number[];
  }

  let { temperature, history }: Props = $props();

  const CHART_MAX_CELSIUS = 100;

  const status = $derived(
    temperature?.state === 'ok' ? statusFor('temperature', temperature.celsius) : 'unavailable',
  );
  const range = $derived(seriesRange(history));
  const chartMax = $derived(Math.max(CHART_MAX_CELSIUS, range?.max ?? 0));
</script>

<MetricCard
  title={t('temperature.title')}
  icon={Thermometer}
  tone="temperature"
  busy={temperature === null || temperature.state === 'loading'}
>
  {#if temperature === null || temperature.state === 'loading'}
    <Skeleton lines={2} label={t('temperature.loading')} />
  {:else if temperature.state === 'unavailable'}
    <div class="metric-headline">
      <span class="metric-reason">{temperature.reason}</span>
      <StatusLabel status="unavailable" />
    </div>
  {:else}
    <div class="metric-headline">
      <span class="metric-value">{formatCelsius(temperature.celsius)}</span>
      <span class="meta">
        <StatusLabel {status} />
        <SourceBadge source={temperature.source} />
      </span>
    </div>
    <Sparkline values={history} tone="temperature" max={chartMax} />
    <p class="sr-only">
      {range === null || history.length < 2
        ? t('chart.collecting')
        : t('chart.summary', { min: formatCelsius(range.min), max: formatCelsius(range.max) })}
    </p>
  {/if}
</MetricCard>

<style>
  .meta {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-4);
  }
</style>
