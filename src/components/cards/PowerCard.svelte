<script lang="ts">
  import Zap from '@lucide/svelte/icons/zap';
  import { formatWatts, powerScopeLabel } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import { seriesRange } from '../../lib/sparkline';
  import type { MetricState, PowerSample } from '../../lib/types';
  import Skeleton from '../primitives/Skeleton.svelte';
  import SourceBadge from '../primitives/SourceBadge.svelte';
  import Sparkline from '../primitives/Sparkline.svelte';
  import StatusLabel from '../primitives/StatusLabel.svelte';
  import MetricCard from './MetricCard.svelte';

  interface Props {
    power: MetricState<PowerSample> | null;
    history: readonly number[];
  }

  let { power, history }: Props = $props();

  const range = $derived(seriesRange(history));
</script>

<MetricCard
  title={t('power.title')}
  icon={Zap}
  tone="power"
  busy={power === null || power.state === 'loading'}
>
  {#if power === null || power.state === 'loading'}
    <Skeleton lines={2} label={t('power.loading')} />
  {:else if power.state === 'unavailable'}
    <div class="metric-headline">
      <span class="metric-reason">{power.reason}</span>
      <StatusLabel status="unavailable" />
    </div>
  {:else}
    <div class="metric-headline">
      <span class="metric-value">{formatWatts(power.smoothedCpuWatts)}</span>
      <span class="meta">
        <span class="metric-secondary">
          {t('power.scopeLabel', { scope: powerScopeLabel(power.scope) })}
        </span>
        <SourceBadge source={power.source} />
      </span>
    </div>
    <Sparkline values={history} tone="power" />
    <p class="sr-only">
      {range === null || history.length < 2
        ? t('chart.collecting')
        : t('chart.summary', { min: formatWatts(range.min), max: formatWatts(range.max) })}
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
