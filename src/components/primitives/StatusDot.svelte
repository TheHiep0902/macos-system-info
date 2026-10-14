<script lang="ts">
  import type { DisplayStatus } from '../../lib/thresholds';

  interface Props {
    status: DisplayStatus;
  }

  let { status }: Props = $props();
</script>

<span class="dot" data-status={status} aria-hidden="true"></span>

<style>
  .dot {
    display: inline-block;
    flex: none;
    width: var(--status-dot-size);
    height: var(--status-dot-size);
    border-radius: var(--radius-pill);
    background: var(--color-status-normal);
  }

  .dot[data-status='warn'] {
    background: var(--color-status-warn);
  }

  .dot[data-status='critical'] {
    background: var(--color-status-critical);
  }

  .dot[data-status='unavailable'] {
    background: var(--color-status-unavailable);
  }
</style>
