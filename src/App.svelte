<script lang="ts">
  import { tick } from 'svelte';
  import CpuCard from './components/cards/CpuCard.svelte';
  import DiskCard from './components/cards/DiskCard.svelte';
  import MemoryCard from './components/cards/MemoryCard.svelte';
  import NetworkCard from './components/cards/NetworkCard.svelte';
  import PowerCard from './components/cards/PowerCard.svelte';
  import TemperatureCard from './components/cards/TemperatureCard.svelte';
  import UpdateBanner from './components/cards/UpdateBanner.svelte';
  import UptimeCard from './components/cards/UptimeCard.svelte';
  import CpuDetailView from './components/cpu/CpuDetailView.svelte';
  import Footer from './components/layout/Footer.svelte';
  import Header from './components/layout/Header.svelte';
  import PopoverShell from './components/layout/PopoverShell.svelte';
  import SettingsView from './components/settings/SettingsView.svelte';
  import { t } from './lib/i18n';
  import type { PopoverStore } from './lib/store.svelte';
  import {
    DARK_SCHEME_QUERY,
    REDUCED_TRANSPARENCY_QUERY,
    resolveEffects,
    resolveTheme,
  } from './lib/theme';
  import type { PopoverView, Settings } from './lib/types';
  import { showUpdateBanner } from './lib/update';

  interface Props {
    store: PopoverStore;
  }

  let { store }: Props = $props();

  let systemPrefersDark = $state(false);
  let systemReducesTransparency = $state(false);
  let cpuButton = $state<HTMLButtonElement | null>(null);
  let settingsButton = $state<HTMLButtonElement | null>(null);

  const notice = $derived(
    store.actionFailed ? t('notice.action') : store.dataFailed ? t('notice.data') : null,
  );

  $effect(() => {
    const darkQuery = window.matchMedia(DARK_SCHEME_QUERY);
    const transparencyQuery = window.matchMedia(REDUCED_TRANSPARENCY_QUERY);
    systemPrefersDark = darkQuery.matches;
    systemReducesTransparency = transparencyQuery.matches;
    const onDark = (event: MediaQueryListEvent) => {
      systemPrefersDark = event.matches;
    };
    const onTransparency = (event: MediaQueryListEvent) => {
      systemReducesTransparency = event.matches;
    };
    darkQuery.addEventListener('change', onDark);
    transparencyQuery.addEventListener('change', onTransparency);
    return () => {
      darkQuery.removeEventListener('change', onDark);
      transparencyQuery.removeEventListener('change', onTransparency);
    };
  });

  $effect(() => {
    const root = document.documentElement;
    root.dataset.theme = resolveTheme(store.settings?.theme ?? 'system', systemPrefersDark);
    root.dataset.effects = resolveEffects(
      store.settings?.reduceEffects ?? false,
      systemReducesTransparency,
    );
    root.dataset.visible = String(store.visible);
  });

  async function navigate(next: PopoverView): Promise<void> {
    const previous = store.view;
    const switching = store.setView(next);
    await tick();
    if (next === 'overview') {
      (previous === 'settings' ? settingsButton : cpuButton)?.focus();
    }
    await switching;
  }

  function resize(height: number): void {
    void store.resize(height);
  }

  function updateSettings(patch: Partial<Settings>): void {
    void store.updateSettings(patch);
  }

  function quit(): void {
    void store.quit();
  }

  function openLoginItemsSettings(): void {
    void store.openLoginItemsSettings();
  }

  function checkForUpdates(): void {
    void store.checkForUpdates();
  }

  function installUpdate(): void {
    void store.installUpdate();
  }

  function onKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' && !event.defaultPrevented) {
      event.preventDefault();
      void store.hide();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if store.view === 'cpu-detail'}
  <CpuDetailView
    snapshot={store.snapshot}
    staticInfo={store.staticInfo}
    {notice}
    onheightchange={resize}
    onback={() => void navigate('overview')}
  />
{:else if store.view === 'settings'}
  <SettingsView
    settings={store.settings}
    saving={store.settingsSaving}
    saveFailed={store.settingsError}
    loadFailed={store.settingsLoadFailed}
    {notice}
    onheightchange={resize}
    onchange={updateSettings}
    onback={() => void navigate('overview')}
    onquit={quit}
    onopenloginitems={openLoginItemsSettings}
    update={store.update}
    oncheckupdates={checkForUpdates}
    oninstallupdate={installUpdate}
  />
{:else}
  <PopoverShell
    {notice}
    onheightchange={resize}
    header={overviewHeader}
    footer={overviewFooter}
    busy={store.snapshot === null}
  >
    {#if store.update !== null && showUpdateBanner(store.update, store.installAttempted)}
      <UpdateBanner
        update={store.update}
        oninstall={installUpdate}
        onopenloginitems={openLoginItemsSettings}
      />
    {/if}
    <CpuCard
      bind:buttonRef={cpuButton}
      snapshot={store.snapshot}
      staticInfo={store.staticInfo}
      history={store.history.cpu}
      onopen={() => void navigate('cpu-detail')}
    />
    <PowerCard power={store.snapshot?.power ?? null} history={store.history.power} />
    <TemperatureCard
      temperature={store.snapshot?.temperature ?? null}
      history={store.history.temperature}
    />
    <MemoryCard memory={store.snapshot?.memory ?? null} />
    <DiskCard disks={store.snapshot?.disks ?? null} changing={store.disksChanging} />
    <NetworkCard
      network={store.snapshot?.network ?? null}
      throughput={store.snapshot?.throughput ?? null}
      rxHistory={store.history.rx}
      txHistory={store.history.tx}
    />
    <UptimeCard uptimeSecs={store.snapshot?.uptimeSecs ?? null} staticInfo={store.staticInfo} />
  </PopoverShell>
{/if}

{#snippet overviewHeader()}
  <Header staticInfo={store.staticInfo} uptimeSecs={store.snapshot?.uptimeSecs ?? null} />
{/snippet}

{#snippet overviewFooter()}
  <Footer
    bind:settingsRef={settingsButton}
    onsettings={() => void navigate('settings')}
    onquit={quit}
  />
{/snippet}
