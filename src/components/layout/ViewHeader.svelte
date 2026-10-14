<script lang="ts">
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import { t } from '../../lib/i18n';
  import IconButton from '../primitives/IconButton.svelte';

  interface Props {
    title: string;
    onback: () => void;
  }

  let { title, onback }: Props = $props();

  let backButton = $state<HTMLButtonElement | null>(null);

  $effect(() => {
    backButton?.focus();
  });
</script>

<div class="view-header">
  <IconButton bind:ref={backButton} label={t('common.back')} icon={ChevronLeft} onclick={onback} />
  <h1 class="title">{title}</h1>
</div>

<style>
  .view-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-left: calc(var(--space-3) * -1);
  }

  .title {
    font-size: var(--font-size-lg);
    font-weight: var(--font-weight-semibold);
    line-height: var(--line-height-tight);
  }
</style>
