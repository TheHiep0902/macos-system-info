<script lang="ts">
  import MemoryStick from '@lucide/svelte/icons/memory-stick';
  import { formatBytes, formatPercent } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import { statusFor } from '../../lib/thresholds';
  import type { MemorySample } from '../../lib/types';
  import ProgressBar from '../primitives/ProgressBar.svelte';
  import Skeleton from '../primitives/Skeleton.svelte';
  import StatusLabel from '../primitives/StatusLabel.svelte';
  import MetricCard from './MetricCard.svelte';

  interface Props {
    memory: MemorySample | null;
  }

  let { memory }: Props = $props();

  const status = $derived(memory === null ? 'normal' : statusFor('memory', memory.usedPct));
  const breakdown = $derived(
    memory === null
      ? []
      : [
          { label: t('memory.app'), value: memory.appBytes },
          { label: t('memory.wired'), value: memory.wiredBytes },
          { label: t('memory.compressed'), value: memory.compressedBytes },
          { label: t('memory.cached'), value: memory.cachedBytes },
          { label: t('memory.swap'), value: memory.swapUsedBytes },
        ],
  );
</script>

<MetricCard title={t('memory.title')} icon={MemoryStick} tone="memory" busy={memory === null}>
  {#if memory === null}
    <Skeleton lines={3} />
  {:else}
    <div class="metric-headline">
      <span class="metric-value">{formatPercent(memory.usedPct)}</span>
      <StatusLabel {status} />
    </div>
    <p class="metric-secondary">
      {t('memory.usage', {
        used: formatBytes(memory.usedBytes),
        total: formatBytes(memory.totalBytes),
      })}
    </p>
    <ProgressBar
      value={memory.usedPct}
      label={t('memory.usageLabel')}
      valueText={formatPercent(memory.usedPct)}
      {status}
    />
    <dl class="metric-list">
      {#each breakdown as row (row.label)}
        <dt>{row.label}</dt>
        <dd>{formatBytes(row.value)}</dd>
      {/each}
    </dl>
  {/if}
</MetricCard>
