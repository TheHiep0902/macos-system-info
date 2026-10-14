<script lang="ts">
  import type { DisplayStatus } from '../../lib/thresholds';

  interface Props {
    value: number;
    label: string;
    valueText?: string;
    status?: DisplayStatus;
    size?: 'md' | 'sm';
  }

  let { value, label, valueText, status = 'normal', size = 'md' }: Props = $props();

  const clamped = $derived(Number.isFinite(value) ? Math.min(100, Math.max(0, value)) : 0);
</script>

<div
  class="track"
  class:small={size === 'sm'}
  role="progressbar"
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={Math.round(clamped)}
  aria-valuetext={valueText}
>
  <div class="fill" data-status={status} style:transform={`scaleX(${clamped / 100})`}></div>
</div>

<style>
  .track {
    position: relative;
    height: var(--progress-height);
    overflow: hidden;
    border-radius: var(--radius-pill);
    background: var(--color-surface-track);
  }

  .track.small {
    height: var(--progress-height-sm);
  }

  .fill {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: var(--color-status-normal);
    transform-origin: left center;
    transition: transform var(--duration-fast) var(--easing-standard);
  }

  .fill[data-status='warn'] {
    background: var(--color-status-warn);
  }

  .fill[data-status='critical'] {
    background: var(--color-status-critical);
  }

  .fill[data-status='unavailable'] {
    background: var(--color-status-unavailable);
  }
</style>
