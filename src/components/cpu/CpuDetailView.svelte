<script lang="ts">
  import { t } from '../../lib/i18n';
  import type { Snapshot, StaticInfo } from '../../lib/types';
  import PopoverShell from '../layout/PopoverShell.svelte';
  import ViewHeader from '../layout/ViewHeader.svelte';
  import Skeleton from '../primitives/Skeleton.svelte';
  import SourceBadge from '../primitives/SourceBadge.svelte';
  import ClusterSummary from './ClusterSummary.svelte';
  import CoreGrid from './CoreGrid.svelte';
  import FrequencyPanel from './FrequencyPanel.svelte';

  interface Props {
    snapshot: Snapshot | null;
    staticInfo: StaticInfo | null;
    notice: string | null;
    onheightchange: (height: number) => void;
    onback: () => void;
  }

  let { snapshot, staticInfo, notice, onheightchange, onback }: Props = $props();

  const uid = $props.id();
  const frequency = $derived(snapshot?.cpu.frequency ?? null);
  const isAppleSilicon = $derived(staticInfo?.platform === 'appleSilicon');
  const showMhz = $derived(frequency?.state === 'ok' && frequency.perCoreAvailable);
  const perCoreUnsupported = $derived(frequency?.state === 'ok' && !frequency.perCoreAvailable);
</script>

<PopoverShell {notice} {onheightchange} header={viewHeader} busy={snapshot === null}>
  {#if snapshot === null || staticInfo === null || frequency === null}
    <Skeleton lines={4} />
  {:else}
    <section class="section" aria-labelledby={`${uid}-topology`}>
      <h2 class="section-title" id={`${uid}-topology`}>
        {t('detail.topology', {
          physical: staticInfo.physicalCores,
          logical: staticInfo.logicalCores,
        })}
      </h2>
      {#if staticInfo.topologyAssumed}
        <p class="metric-note">{t('detail.topologyAssumed')}</p>
      {/if}
    </section>

    <section class="section" aria-labelledby={`${uid}-frequency`}>
      <div class="section-header">
        <h2 class="section-title" id={`${uid}-frequency`}>
          {isAppleSilicon ? t('detail.clusters') : t('freq.title')}
        </h2>
        {#if frequency.state === 'ok'}
          <SourceBadge source={frequency.source} />
        {/if}
      </div>
      {#if isAppleSilicon}
        <ClusterSummary clusters={snapshot.cpu.clusters} info={staticInfo.clusters} {frequency} />
      {:else}
        <FrequencyPanel {frequency} {staticInfo} />
      {/if}
    </section>

    <section class="section" aria-labelledby={`${uid}-cores`}>
      <h2 class="section-title" id={`${uid}-cores`}>
        {isAppleSilicon ? t('detail.cores') : t('detail.threads')}
      </h2>
      {#if perCoreUnsupported}
        <p class="metric-note">{t('detail.perCoreUnsupported')}</p>
      {/if}
      <CoreGrid
        cores={snapshot.cpu.cores}
        clusters={staticInfo.clusters}
        {showMhz}
        groupByCluster={isAppleSilicon}
        labelKind={isAppleSilicon ? 'core' : 'cpu'}
      />
    </section>
  {/if}
</PopoverShell>

{#snippet viewHeader()}
  <ViewHeader title={t('detail.title')} {onback} />
{/snippet}

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .section + .section {
    padding-top: var(--space-4);
    border-top: var(--border-width) solid var(--color-border-subtle);
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .section-title {
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-semibold);
  }
</style>
