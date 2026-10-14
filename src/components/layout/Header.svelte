<script lang="ts">
  import { formatUptime } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import type { StaticInfo } from '../../lib/types';
  import Badge from '../primitives/Badge.svelte';

  interface Props {
    staticInfo: StaticInfo | null;
    uptimeSecs: number | null;
  }

  let { staticInfo, uptimeSecs }: Props = $props();

  const platformLabel = $derived(
    staticInfo === null
      ? null
      : staticInfo.platform === 'appleSilicon'
        ? t('header.platform.appleSilicon')
        : t('header.platform.intel'),
  );
</script>

<div class="header">
  <div class="title-row">
    <h1 class="brand">{staticInfo?.cpuBrand ?? t('app.name')}</h1>
    {#if platformLabel !== null}
      <div class="badges">
        <Badge tone="accent">{platformLabel}</Badge>
        {#if staticInfo?.translated}
          <Badge tone="warn">{t('header.rosetta')}</Badge>
        {/if}
      </div>
    {/if}
  </div>
  {#if staticInfo?.translated}
    <p class="warning">{t('header.rosettaHint')}</p>
  {/if}
  <p class="meta">
    {uptimeSecs === null
      ? t('header.starting')
      : t('header.uptime', { uptime: formatUptime(uptimeSecs) })}
  </p>
</div>

<style>
  .header {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .title-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3) var(--space-4);
  }

  .brand {
    min-width: 0;
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    line-height: var(--line-height-tight);
    overflow-wrap: anywhere;
  }

  .badges {
    display: flex;
    gap: var(--space-2);
  }

  .warning {
    color: var(--color-status-warn);
    font-size: var(--font-size-sm);
  }

  .meta {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
  }
</style>
