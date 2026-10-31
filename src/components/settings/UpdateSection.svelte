<script lang="ts">
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Download from '@lucide/svelte/icons/download';
  import { t } from '../../lib/i18n';
  import type { UpdateState } from '../../lib/types';
  import { lastCheckText, updateView } from '../../lib/update';
  import ProgressBar from '../primitives/ProgressBar.svelte';
  import SettingSwitch from './SettingSwitch.svelte';

  interface Props {
    update: UpdateState | null;
    autoCheck: boolean;
    onautocheckchange: (autoCheckUpdates: boolean) => void;
    oncheck: () => void;
    oninstall: () => void;
  }

  let { update, autoCheck, onautocheckchange, oncheck, oninstall }: Props = $props();

  const uid = $props.id();

  const REFRESH_MS = 30_000;

  const view = $derived(update === null ? null : updateView(update));

  let now = $state(Date.now());

  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), REFRESH_MS);
    return () => clearInterval(timer);
  });

  const lastChecked = $derived(
    update === null ? null : lastCheckText(update.lastCheckAt, Math.max(now, Date.now())),
  );
</script>

<section class="section" aria-labelledby={`${uid}-title`}>
  <h2 class="section-title" id={`${uid}-title`}>{t('update.section')}</h2>
  {#if update !== null}
    <p class="version">{t('update.currentVersion', { version: update.currentVersion })}</p>
  {/if}
  <p class="status" data-tone={view?.tone ?? 'neutral'} role="status">{view?.statusText ?? ''}</p>
  {#if view?.errorText}
    <p class="error" role="alert">{view.errorText}</p>
  {/if}
  {#if view?.hint}
    <p class="hint">{view.hint}</p>
  {/if}
  {#if view?.notes}
    <div class="notes">
      <h3 class="notes-title" id={`${uid}-notes`}>{t('update.notes')}</h3>
      <p class="notes-text">{view.notes}</p>
    </div>
  {/if}
  {#if view?.progress}
    {#if view.progress.value !== null}
      <ProgressBar
        value={view.progress.value}
        label={t('update.progressLabel')}
        valueText={view.progress.text}
        size="sm"
      />
    {/if}
    <p class="hint">{view.progress.text}</p>
  {/if}
  <div class="actions">
    {#if view?.showInstall}
      <button type="button" class="action primary" onclick={oninstall}>
        <Download class="action-icon" aria-hidden="true" />
        {t('update.install')}
      </button>
    {/if}
    <button
      type="button"
      class="action"
      disabled={update === null || view?.canCheck === false}
      onclick={oncheck}
    >
      <RefreshCw class="action-icon" aria-hidden="true" />
      {t('update.check')}
    </button>
  </div>
  {#if lastChecked !== null}
    <p class="hint">{lastChecked}</p>
  {/if}
  <SettingSwitch
    label={t('update.autoCheck')}
    hint={t('update.autoCheckHint')}
    checked={autoCheck}
    onchange={onautocheckchange}
  />
</section>

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-5) var(--space-6);
    border: var(--border-width) solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
    background: var(--color-surface-card);
  }

  .section-title {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-semibold);
  }

  .version {
    color: var(--color-text-primary);
    font-size: var(--font-size-md);
  }

  .status {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
  }

  .status:empty {
    display: none;
  }

  .status[data-tone='accent'] {
    color: var(--color-text-primary);
    font-weight: var(--font-weight-medium);
  }

  .status[data-tone='warn'] {
    color: var(--color-status-warn);
  }

  .status[data-tone='critical'] {
    color: var(--color-status-critical);
  }

  .error {
    color: var(--color-status-critical);
    font-size: var(--font-size-sm);
  }

  .hint {
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
  }

  .notes {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-4);
    border: var(--border-width) solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .notes-title {
    color: var(--color-text-secondary);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
  }

  .notes-text {
    color: var(--color-text-primary);
    font-size: var(--font-size-sm);
    white-space: pre-line;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .action {
    display: inline-flex;
    align-self: flex-start;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--control-size);
    padding: var(--space-2) var(--space-6);
    border: var(--border-width) solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    background: var(--color-surface-card);
    color: var(--color-text-primary);
    font-size: var(--font-size-md);
    transition: background-color var(--duration-fast) var(--easing-standard);
  }

  .action.primary {
    border-color: var(--color-accent);
    color: var(--color-text-badge-accent);
  }

  .action:hover:not(:disabled) {
    background: var(--color-surface-hover);
  }

  .action:active:not(:disabled) {
    background: var(--color-surface-active);
  }

  .action:disabled {
    opacity: var(--opacity-disabled);
  }

  .action :global(.action-icon) {
    flex: none;
    width: var(--icon-size-sm);
    height: var(--icon-size-sm);
  }
</style>
