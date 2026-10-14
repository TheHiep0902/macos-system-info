<script lang="ts">
  import { clusterName } from '../../lib/format';
  import { t } from '../../lib/i18n';
  import type { ClusterInfo, CoreSample } from '../../lib/types';
  import Skeleton from '../primitives/Skeleton.svelte';
  import CoreCell from './CoreCell.svelte';

  interface Props {
    cores: readonly CoreSample[];
    clusters: readonly ClusterInfo[];
    showMhz: boolean;
    groupByCluster: boolean;
    labelKind: 'core' | 'cpu';
  }

  let { cores, clusters, showMhz, groupByCluster, labelKind }: Props = $props();

  interface CoreGroup {
    key: string;
    heading: string | null;
    cores: CoreSample[];
  }

  const groups = $derived.by((): CoreGroup[] => {
    const sorted = [...cores].sort((left, right) => left.id - right.id);
    if (!groupByCluster) {
      return [{ key: 'all', heading: null, cores: sorted }];
    }
    const order = clusters.map((cluster) => cluster.label);
    for (const core of sorted) {
      if (!order.includes(core.cluster)) {
        order.push(core.cluster);
      }
    }
    return order
      .map((label) => {
        const members = sorted.filter((core) => core.cluster === label);
        return {
          key: label,
          heading: t('detail.groupHeading', {
            name: clusterName(label, clusters),
            count: members.length,
          }),
          cores: members,
        };
      })
      .filter((group) => group.cores.length > 0);
  });

  function coreLabel(id: number): string {
    return labelKind === 'core' ? t('detail.coreLabel', { id }) : t('detail.cpuLabel', { id });
  }
</script>

{#if cores.length === 0}
  <Skeleton lines={3} label={t('detail.coresLoading')} />
{:else}
  <div class="groups">
    {#each groups as group (group.key)}
      <div class="group">
        {#if group.heading !== null}
          <h3 class="heading">{group.heading}</h3>
        {/if}
        <ul class="grid">
          {#each group.cores as core (core.id)}
            <li>
              <CoreCell {core} label={coreLabel(core.id)} {showMhz} />
            </li>
          {/each}
        </ul>
      </div>
    {/each}
  </div>
{/if}

<style>
  .groups {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .heading {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-semibold);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(var(--core-cell-min-width), 1fr));
    gap: var(--space-3);
  }
</style>
