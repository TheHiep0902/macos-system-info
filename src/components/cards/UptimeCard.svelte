<script lang="ts">
  import Clock from '@lucide/svelte/icons/clock';
  import { formatDateTime, formatUptime } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import type { StaticInfo } from '../../lib/types';
  import Skeleton from '../primitives/Skeleton.svelte';
  import MetricCard from './MetricCard.svelte';

  interface Props {
    uptimeSecs: number | null;
    staticInfo: StaticInfo | null;
  }

  let { uptimeSecs, staticInfo }: Props = $props();
</script>

<MetricCard title={t('uptime.title')} icon={Clock} tone="uptime" busy={uptimeSecs === null}>
  {#if uptimeSecs === null}
    <Skeleton lines={2} />
  {:else}
    <p class="value">{formatUptime(uptimeSecs)}</p>
    {#if staticInfo !== null}
      <dl class="metric-list">
        <dt>{t('uptime.bootedAt')}</dt>
        <dd>{formatDateTime(staticInfo.bootTimeUnix)}</dd>
        <dt>{t('uptime.os')}</dt>
        <dd>{staticInfo.osVersion}</dd>
        <dt>{t('uptime.model')}</dt>
        <dd>{staticInfo.modelIdentifier}</dd>
      </dl>
    {/if}
  {/if}
</MetricCard>

<style>
  .value {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
  }
</style>
