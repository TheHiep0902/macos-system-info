<script lang="ts">
  import ArrowDown from '@lucide/svelte/icons/arrow-down';
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import Wifi from '@lucide/svelte/icons/wifi';
  import WifiOff from '@lucide/svelte/icons/wifi-off';
  import { formatRate } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import { seriesRange } from '../../lib/sparkline';
  import type { MetricState, NetworkSample, ThroughputSample } from '../../lib/types';
  import EmptyState from '../primitives/EmptyState.svelte';
  import Skeleton from '../primitives/Skeleton.svelte';
  import Sparkline from '../primitives/Sparkline.svelte';
  import StatusLabel from '../primitives/StatusLabel.svelte';
  import MetricCard from './MetricCard.svelte';

  interface Props {
    network: NetworkSample | null;
    throughput: MetricState<ThroughputSample> | null;
    rxHistory: readonly number[];
    txHistory: readonly number[];
  }

  let { network, throughput, rxHistory, txHistory }: Props = $props();

  const BYTES_PER_MEGABYTE = 1_000_000;

  const hasAddress = $derived(
    network !== null && (network.wifiIpv4 !== null || network.wifiIpv6 !== null),
  );

  const rates = $derived(
    throughput?.state === 'ok'
      ? [
          {
            key: 'rx',
            label: t('network.download'),
            icon: ArrowDown,
            value: throughput.rxBytesPerSec,
            history: rxHistory,
            range: seriesRange(rxHistory),
          },
          {
            key: 'tx',
            label: t('network.upload'),
            icon: ArrowUp,
            value: throughput.txBytesPerSec,
            history: txHistory,
            range: seriesRange(txHistory),
          },
        ]
      : [],
  );
</script>

<MetricCard
  title={t('network.title')}
  icon={Wifi}
  tone="network"
  busy={network === null || throughput === null || throughput.state === 'loading'}
>
  {#if throughput === null || throughput.state === 'loading'}
    <Skeleton lines={2} />
  {:else if throughput.state === 'unavailable'}
    <div class="metric-headline">
      <span class="metric-reason">{throughput.reason || t('network.rateUnavailable')}</span>
      <StatusLabel status="unavailable" />
    </div>
  {:else}
    <dl class="rates">
      {#each rates as rate (rate.key)}
        <div class="rate">
          <dt class="metric-secondary">
            <rate.icon class="rate-icon" aria-hidden="true" />
            {rate.label}
          </dt>
          <dd class="rate-value">{formatRate(rate.value)}</dd>
          <dd>
            <Sparkline values={rate.history} tone="network" max={rate.range?.max ?? null} />
            <span class="sr-only">
              {rate.range === null || rate.history.length < 2
                ? t('chart.collecting')
                : t('chart.summary', {
                    min: formatRate(rate.range.min * BYTES_PER_MEGABYTE),
                    max: formatRate(rate.range.max * BYTES_PER_MEGABYTE),
                  })}
            </span>
          </dd>
        </div>
      {/each}
    </dl>
  {/if}
  {#if network === null}
    <Skeleton lines={2} />
  {:else if !network.isWifiConnected}
    <EmptyState
      icon={WifiOff}
      title={t('network.disconnected')}
      description={network.primaryInterface === null
        ? undefined
        : t('network.primary', { iface: network.primaryInterface })}
    />
  {:else}
    <dl class="metric-list">
      {#if network.wifiInterface !== null}
        <dt>{t('network.interface')}</dt>
        <dd>{network.wifiInterface}</dd>
      {/if}
      {#if network.wifiIpv4 !== null}
        <dt>{t('network.ipv4')}</dt>
        <dd class="selectable">{network.wifiIpv4}</dd>
      {/if}
      {#if network.wifiIpv6 !== null}
        <dt>{t('network.ipv6')}</dt>
        <dd class="selectable">{network.wifiIpv6}</dd>
      {/if}
    </dl>
    {#if !hasAddress}
      <p class="metric-note">{t('network.noAddress')}</p>
    {/if}
  {/if}
</MetricCard>

<style>
  .rates {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-4);
  }

  .rate {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  .rate dt {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }

  .rate :global(.rate-icon) {
    flex: none;
    width: var(--icon-size-sm);
    height: var(--icon-size-sm);
  }

  .rate-value {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    font-variant-numeric: tabular-nums;
    line-height: var(--line-height-tight);
    white-space: nowrap;
  }
</style>
