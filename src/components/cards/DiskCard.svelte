<script lang="ts">
  import HardDrive from '@lucide/svelte/icons/hard-drive';
  import { formatBytes, formatPercent } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import { fadeToken } from '../../lib/motion';
  import { statusFor } from '../../lib/thresholds';
  import type { DiskSample } from '../../lib/types';
  import Badge from '../primitives/Badge.svelte';
  import EmptyState from '../primitives/EmptyState.svelte';
  import ProgressBar from '../primitives/ProgressBar.svelte';
  import Skeleton from '../primitives/Skeleton.svelte';
  import StatusLabel from '../primitives/StatusLabel.svelte';
  import MetricCard from './MetricCard.svelte';

  interface Props {
    disks: readonly DiskSample[] | null;
    changing: boolean;
  }

  let { disks, changing }: Props = $props();

  const ordered = $derived(
    disks === null
      ? []
      : [...disks.filter((disk) => disk.isBoot), ...disks.filter((disk) => !disk.isBoot)],
  );
</script>

<MetricCard title={t('disk.title')} icon={HardDrive} tone="disk" busy={disks === null || changing}>
  <p class="sr-only" role="status">{changing ? t('disk.updating') : ''}</p>
  {#if disks === null}
    <Skeleton lines={2} />
  {:else if ordered.length === 0}
    <EmptyState title={t('disk.empty')} />
  {:else}
    <ul class="disks">
      {#each ordered as disk (disk.id)}
        {@const status = statusFor('disk', disk.usedPct)}
        <li class="disk" in:fadeToken out:fadeToken>
          <div class="row">
            <span class="name" title={disk.mountPoint}>{disk.name}</span>
            <span class="tags">
              {#if disk.isBoot}
                <Badge>{t('disk.boot')}</Badge>
              {/if}
              {#if disk.isRemovable}
                <Badge tone="accent">{t('disk.removable')}</Badge>
              {/if}
              {#if disk.isNetwork}
                <Badge tone="accent">{t('disk.network')}</Badge>
              {/if}
              {#if status !== 'normal'}
                <StatusLabel {status} />
              {/if}
            </span>
          </div>
          <ProgressBar
            value={disk.usedPct}
            size="sm"
            label={t('disk.usageLabel', { name: disk.name })}
            valueText={formatPercent(disk.usedPct)}
            {status}
          />
          <div class="row">
            <span class="metric-secondary">
              {t('disk.usage', {
                used: formatBytes(disk.usedBytes),
                total: formatBytes(disk.totalBytes),
              })}
            </span>
            <span class="metric-secondary">
              {disk.availableImportantBytes === null
                ? t('disk.free', { size: formatBytes(disk.availableBytes) })
                : t('disk.available', { size: formatBytes(disk.availableImportantBytes) })}
            </span>
          </div>
          {#if disk.availableImportantBytes !== null}
            <p class="metric-note">{t('disk.purgeableNote')}</p>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</MetricCard>

<style>
  .disks {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .disk {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .disk + .disk {
    padding-top: var(--space-4);
    border-top: var(--border-width) solid var(--color-border-subtle);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .name {
    min-width: 0;
    overflow: hidden;
    font-weight: var(--font-weight-medium);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tags {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: var(--space-2);
  }
</style>
