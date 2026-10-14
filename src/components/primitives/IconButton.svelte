<script lang="ts">
  import type { LucideIcon } from '@lucide/svelte';

  interface Props {
    label: string;
    icon: LucideIcon;
    onclick: () => void;
    disabled?: boolean;
    ref?: HTMLButtonElement | null;
  }

  let { label, icon: Icon, onclick, disabled = false, ref = $bindable(null) }: Props = $props();
</script>

<button
  bind:this={ref}
  type="button"
  class="icon-button"
  aria-label={label}
  title={label}
  {disabled}
  {onclick}
>
  <Icon class="icon" aria-hidden="true" />
</button>

<style>
  .icon-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--control-size);
    height: var(--control-size);
    padding: 0;
    border: 0;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--color-text-secondary);
    transition: background-color var(--duration-fast) var(--easing-standard);
  }

  .icon-button:hover:not(:disabled) {
    background: var(--color-surface-hover);
    color: var(--color-text-primary);
  }

  .icon-button:active:not(:disabled) {
    background: var(--color-surface-active);
  }

  .icon-button:disabled {
    color: var(--color-text-tertiary);
  }

  .icon-button :global(.icon) {
    width: var(--icon-size-md);
    height: var(--icon-size-md);
  }
</style>
