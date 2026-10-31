<script lang="ts">
  import CircleCheckBig from '@lucide/svelte/icons/circle-check-big';
  import Download from '@lucide/svelte/icons/download';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import { t } from '../../lib/i18n';
  import type { UpdateState } from '../../lib/types';
  import { notesAreLong, postUpdateView, updateView } from '../../lib/update';
  import ProgressBar from '../primitives/ProgressBar.svelte';

  interface Props {
    update: UpdateState;
    oninstall: () => void;
    onopenloginitems: () => void;
  }

  let { update, oninstall, onopenloginitems }: Props = $props();

  const uid = $props.id();

  let expanded = $state(false);

  const view = $derived(updateView(update));
  const post = $derived(postUpdateView(update));
  const PHASE_KINDS: readonly string[] = [
    'available',
    'downloading',
    'installing',
    'failed',
    'blocked',
  ];

  const showPhase = $derived(PHASE_KINDS.includes(update.phase.kind));
  const longNotes = $derived(view.notes !== null && notesAreLong(view.notes));
</script>

<section class="banner" aria-labelledby={`${uid}-title`}>
  {#if post !== null}
    <div class="row">
      <CircleCheckBig class="banner-icon" aria-hidden="true" />
      <h2 class="title" id={`${uid}-title`}>{post.title}</h2>
    </div>
    {#if post.loginText !== null}
      <p class="hint">{post.loginText}</p>
    {/if}
    {#if post.showOpenSettings}
      <button type="button" class="action" onclick={onopenloginitems}>
        <ExternalLink class="action-icon" aria-hidden="true" />
        {t('settings.launchAtLogin.openSettings')}
      </button>
    {/if}
  {/if}
  {#if showPhase}
    <div class="row">
      <Download class="banner-icon" aria-hidden="true" />
      <p class="title" id={post === null ? `${uid}-title` : undefined} role="status">
        {view.statusText}
      </p>
    </div>
    {#if view.notes !== null}
      <p class="notes" class:collapsed={longNotes && !expanded} id={`${uid}-notes`}>
        {view.notes}
      </p>
      {#if longNotes}
        <button
          type="button"
          class="toggle"
          aria-expanded={expanded}
          aria-controls={`${uid}-notes`}
          onclick={() => (expanded = !expanded)}
        >
          {expanded ? t('update.notes.hide') : t('update.notes.show')}
        </button>
      {/if}
    {/if}
    {#if view.progress !== null}
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
    {#if view.hint !== null}
      <p class="hint">{view.hint}</p>
    {/if}
    {#if view.errorText !== null}
      <p class="error" role="alert">{view.errorText}</p>
    {/if}
    {#if view.showInstall}
      <button type="button" class="action primary" onclick={oninstall}>
        <Download class="action-icon" aria-hidden="true" />
        {t('update.install')}
      </button>
    {/if}
  {/if}
</section>

<style>
  .banner {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-5) var(--space-6);
    border: var(--border-width) solid var(--color-accent);
    border-radius: var(--radius-lg);
    background: var(--color-surface-card);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .banner :global(.banner-icon) {
    flex: none;
    width: var(--icon-size-sm);
    height: var(--icon-size-sm);
    color: var(--color-accent);
  }

  .title {
    color: var(--color-text-primary);
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-semibold);
  }

  .notes {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
    white-space: pre-line;
    overflow-wrap: anywhere;
  }

  .notes.collapsed {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
  }

  .hint {
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
  }

  .error {
    color: var(--color-status-critical);
    font-size: var(--font-size-sm);
  }

  .toggle {
    align-self: flex-start;
    padding: 0;
    border: none;
    background: none;
    color: var(--color-text-badge-accent);
    font-size: var(--font-size-sm);
    text-decoration: underline;
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

  .action:hover {
    background: var(--color-surface-hover);
  }

  .action:active {
    background: var(--color-surface-active);
  }

  .banner :global(.action-icon) {
    flex: none;
    width: var(--icon-size-sm);
    height: var(--icon-size-sm);
  }
</style>
