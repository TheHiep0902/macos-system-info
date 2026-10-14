<script lang="ts">
  import Power from '@lucide/svelte/icons/power';
  import Settings from '@lucide/svelte/icons/settings';
  import { t } from '../../lib/i18n';
  import IconButton from '../primitives/IconButton.svelte';

  interface Props {
    onsettings: () => void;
    onquit: () => void;
    settingsRef?: HTMLButtonElement | null;
  }

  let { onsettings, onquit, settingsRef = $bindable(null) }: Props = $props();
</script>

<div class="footer">
  <span class="name">{t('app.name')}</span>
  <div class="actions">
    <IconButton
      bind:ref={settingsRef}
      label={t('footer.settings')}
      icon={Settings}
      onclick={onsettings}
    />
    <IconButton label={t('footer.quit')} icon={Power} onclick={onquit} />
  </div>
</div>

<style>
  .footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .name {
    padding-left: var(--space-2);
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
  }

  .actions {
    display: flex;
    gap: var(--space-2);
  }
</style>
