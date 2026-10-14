<script lang="ts">
  import { statusLabel, type DisplayStatus } from '../../lib/thresholds';
  import StatusDot from './StatusDot.svelte';

  interface Props {
    status: DisplayStatus;
  }

  let { status }: Props = $props();
</script>

<span class="status" data-status={status}>
  <StatusDot {status} />
  {statusLabel(status)}
</span>

<style>
  .status {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    white-space: nowrap;
  }

  .status[data-status='warn'] {
    color: var(--color-status-warn);
  }

  .status[data-status='critical'] {
    color: var(--color-status-critical);
  }

  .status[data-status='unavailable'] {
    color: var(--color-status-unavailable);
  }
</style>
