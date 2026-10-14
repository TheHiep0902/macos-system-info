<script lang="ts">
  import Wifi from '@lucide/svelte/icons/wifi';
  import WifiOff from '@lucide/svelte/icons/wifi-off';
  import { t } from '../../lib/i18n';
  import type { NetworkSample } from '../../lib/types';
  import EmptyState from '../primitives/EmptyState.svelte';
  import Skeleton from '../primitives/Skeleton.svelte';
  import MetricCard from './MetricCard.svelte';

  interface Props {
    network: NetworkSample | null;
  }

  let { network }: Props = $props();

  const hasAddress = $derived(
    network !== null && (network.wifiIpv4 !== null || network.wifiIpv6 !== null),
  );
</script>

<MetricCard title={t('network.title')} icon={Wifi} tone="network" busy={network === null}>
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
