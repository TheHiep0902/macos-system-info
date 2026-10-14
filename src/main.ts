import './styles/tokens.css';
import './styles/themes.css';
import './styles/base.css';
import { mount } from 'svelte';
import App from './App.svelte';
import { PopoverStore } from './lib/store.svelte';
import { DARK_SCHEME_QUERY, resolveTheme } from './lib/theme';
import { createTransport } from './lib/transport';

async function start(target: HTMLElement): Promise<void> {
  document.documentElement.dataset.theme = resolveTheme(
    'system',
    window.matchMedia(DARK_SCHEME_QUERY).matches,
  );
  const { transport, initialView } = await createTransport();
  const store = new PopoverStore(transport, initialView);
  mount(App, { target, props: { store } });
  await store.init();
}

const target = document.getElementById('app');
if (!target) {
  throw new Error('Missing #app mount point');
}

start(target).catch((error: unknown) => {
  console.error('[pulsebar] failed to start', error);
});
