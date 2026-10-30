<script lang="ts">
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import Power from '@lucide/svelte/icons/power';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import { t } from '../../lib/i18n';
  import { launchAtLoginUi } from '../../lib/launch-at-login';
  import { menuBarLocks, type TitleMetricKey } from '../../lib/title-metrics';
  import type { Settings, ThemePreference, TitleFormat } from '../../lib/types';
  import PopoverShell from '../layout/PopoverShell.svelte';
  import ViewHeader from '../layout/ViewHeader.svelte';
  import EmptyState from '../primitives/EmptyState.svelte';
  import Skeleton from '../primitives/Skeleton.svelte';
  import SettingRadioGroup from './SettingRadioGroup.svelte';
  import SettingSwitch from './SettingSwitch.svelte';

  interface Props {
    settings: Settings | null;
    saving: boolean;
    saveFailed: boolean;
    loadFailed: boolean;
    notice: string | null;
    onheightchange: (height: number) => void;
    onchange: (patch: Partial<Settings>) => void;
    onback: () => void;
    onquit: () => void;
    onopenloginitems: () => void;
  }

  let {
    settings,
    saving,
    saveFailed,
    loadFailed,
    notice,
    onheightchange,
    onchange,
    onback,
    onquit,
    onopenloginitems,
  }: Props = $props();

  const uid = $props.id();

  const titleFormats: readonly { value: TitleFormat; label: string }[] = [
    { value: 'default', label: t('settings.titleFormat.default') },
    { value: 'compact', label: t('settings.titleFormat.compact') },
  ];

  const themes: readonly { value: ThemePreference; label: string }[] = [
    { value: 'system', label: t('settings.theme.system') },
    { value: 'light', label: t('settings.theme.light') },
    { value: 'dark', label: t('settings.theme.dark') },
  ];

  const titleMetrics: readonly { key: TitleMetricKey; label: string }[] = [
    { key: 'cpu', label: t('settings.titleMetrics.cpu') },
    { key: 'power', label: t('settings.titleMetrics.power') },
    { key: 'memory', label: t('settings.titleMetrics.memory') },
    { key: 'temperature', label: t('settings.titleMetrics.temperature') },
    { key: 'network', label: t('settings.titleMetrics.network') },
  ];

  function setTitleMetric(key: TitleMetricKey, shown: boolean): void {
    if (settings === null) {
      return;
    }
    onchange({ titleMetrics: { ...settings.titleMetrics, [key]: shown } });
  }
</script>

<PopoverShell {notice} {onheightchange} header={viewHeader} busy={settings === null}>
  {#if settings === null}
    {#if loadFailed}
      <EmptyState icon={TriangleAlert} tone="error" title={t('settings.loadError')} />
    {:else}
      <Skeleton lines={5} />
    {/if}
  {:else}
    {@const locks = menuBarLocks(settings)}
    {@const launch = launchAtLoginUi(settings)}
    <section class="section" aria-labelledby={`${uid}-menu-bar`}>
      <h2 class="section-title" id={`${uid}-menu-bar`}>{t('settings.menuBar')}</h2>
      <SettingRadioGroup
        legend={t('settings.titleFormat')}
        name="title-format"
        value={settings.titleFormat}
        options={titleFormats}
        onchange={(titleFormat) => onchange({ titleFormat })}
      />
      <div class="metrics" role="group" aria-labelledby={`${uid}-title-metrics`}>
        <span id={`${uid}-title-metrics`}>{t('settings.titleMetrics')}</span>
        <div class="metric-switches">
          {#each titleMetrics as metric (metric.key)}
            <SettingSwitch
              label={metric.label}
              hint={locks.metrics[metric.key] ? t('settings.titleMetrics.hint') : undefined}
              checked={settings.titleMetrics[metric.key]}
              disabled={locks.metrics[metric.key]}
              onchange={(shown) => setTitleMetric(metric.key, shown)}
            />
          {/each}
        </div>
      </div>
      <SettingSwitch
        label={t('settings.showTrayIcon')}
        hint={locks.icon ? t('settings.titleMetrics.hint') : undefined}
        checked={settings.showTrayIcon}
        disabled={locks.icon}
        onchange={(showTrayIcon) => onchange({ showTrayIcon })}
      />
    </section>

    <section class="section" aria-labelledby={`${uid}-appearance`}>
      <h2 class="section-title" id={`${uid}-appearance`}>{t('settings.appearance')}</h2>
      <SettingRadioGroup
        legend={t('settings.theme')}
        name="theme"
        value={settings.theme}
        options={themes}
        onchange={(theme) => onchange({ theme })}
      />
      <SettingSwitch
        label={t('settings.reduceEffects')}
        checked={settings.reduceEffects}
        onchange={(reduceEffects) => onchange({ reduceEffects })}
      />
    </section>

    <section class="section" aria-labelledby={`${uid}-disks`}>
      <h2 class="section-title" id={`${uid}-disks`}>{t('settings.disks')}</h2>
      <SettingSwitch
        label={t('settings.showNetworkDisks')}
        checked={settings.showNetworkDisks}
        onchange={(showNetworkDisks) => onchange({ showNetworkDisks })}
      />
    </section>

    <section class="section" aria-labelledby={`${uid}-system`}>
      <h2 class="section-title" id={`${uid}-system`}>{t('settings.system')}</h2>
      <SettingSwitch
        label={t('settings.memorySaver')}
        hint={t('settings.memorySaverHint')}
        checked={settings.memorySaver}
        onchange={(memorySaver) => onchange({ memorySaver })}
      />
      <SettingSwitch
        label={t('settings.launchAtLogin')}
        hint={launch.hint}
        checked={settings.launchAtLogin}
        disabled={launch.disabled}
        onchange={(launchAtLogin) => onchange({ launchAtLogin })}
      />
      {#if launch.showOpenSettings}
        <button type="button" class="open-settings" onclick={onopenloginitems}>
          <ExternalLink class="open-settings-icon" aria-hidden="true" />
          {t('settings.launchAtLogin.openSettings')}
        </button>
      {/if}
    </section>

    <p class="metric-note" role="status">{saving ? t('settings.saving') : ''}</p>
    {#if saveFailed}
      <p class="error" role="alert">{t('settings.saveError')}</p>
    {/if}

    <button type="button" class="quit" onclick={onquit}>
      <Power class="quit-icon" aria-hidden="true" />
      {t('settings.quit')}
    </button>
  {/if}
</PopoverShell>

{#snippet viewHeader()}
  <ViewHeader title={t('settings.title')} {onback} />
{/snippet}

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

  .metrics,
  .metric-switches {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .metric-switches {
    padding-inline-start: var(--space-6);
  }

  .error {
    color: var(--color-status-critical);
    font-size: var(--font-size-sm);
  }

  .open-settings {
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

  .open-settings:hover {
    background: var(--color-surface-hover);
  }

  .open-settings:active {
    background: var(--color-surface-active);
  }

  .open-settings :global(.open-settings-icon) {
    flex: none;
    width: var(--icon-size-sm);
    height: var(--icon-size-sm);
  }

  .quit {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    min-height: var(--control-size);
    padding: var(--space-2) var(--space-6);
    border: var(--border-width) solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    background: var(--color-surface-card);
    color: var(--color-status-critical);
    font-size: var(--font-size-md);
    transition: background-color var(--duration-fast) var(--easing-standard);
  }

  .quit:hover {
    background: var(--color-surface-hover);
  }

  .quit:active {
    background: var(--color-surface-active);
  }

  .quit :global(.quit-icon) {
    width: var(--icon-size-sm);
    height: var(--icon-size-sm);
  }
</style>
