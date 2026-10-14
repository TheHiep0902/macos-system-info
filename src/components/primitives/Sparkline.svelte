<script lang="ts">
  import { SPARKLINE_HEIGHT, sparklinePoints } from '../../lib/sparkline';

  interface Props {
    values: readonly number[];
    tone: 'cpu' | 'power' | 'memory';
    capacity?: number;
    max?: number | null;
  }

  let { values, tone, capacity = 60, max = null }: Props = $props();

  const points = $derived(sparklinePoints(values, capacity, max));
  const viewBox = $derived(`0 0 ${Math.max(1, capacity - 1)} ${SPARKLINE_HEIGHT}`);
</script>

<svg
  class="sparkline"
  data-tone={tone}
  {viewBox}
  preserveAspectRatio="none"
  aria-hidden="true"
  focusable="false"
>
  <line class="baseline" x1="0" x2={capacity - 1} y1={SPARKLINE_HEIGHT} y2={SPARKLINE_HEIGHT} />
  {#if points !== ''}
    <polyline {points} />
  {/if}
</svg>

<style>
  .sparkline {
    display: block;
    width: 100%;
    height: var(--sparkline-height);
    overflow: visible;
  }

  .baseline {
    stroke: var(--color-border-subtle);
    stroke-width: var(--size-hairline);
    vector-effect: non-scaling-stroke;
  }

  polyline {
    fill: none;
    stroke: var(--color-chart-cpu);
    stroke-width: var(--sparkline-stroke);
    stroke-linejoin: round;
    stroke-linecap: round;
    vector-effect: non-scaling-stroke;
  }

  [data-tone='power'] polyline {
    stroke: var(--color-chart-power);
  }

  [data-tone='memory'] polyline {
    stroke: var(--color-chart-memory);
  }
</style>
