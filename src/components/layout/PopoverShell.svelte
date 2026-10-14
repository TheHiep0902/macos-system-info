<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    header?: Snippet;
    footer?: Snippet;
    children: Snippet;
    notice?: string | null;
    busy?: boolean;
    onheightchange: (height: number) => void;
  }

  let { header, footer, children, notice = null, busy = false, onheightchange }: Props = $props();

  let headerElement = $state<HTMLElement | null>(null);
  let bodyElement = $state<HTMLElement | null>(null);
  let contentElement = $state<HTMLElement | null>(null);
  let footerElement = $state<HTMLElement | null>(null);

  const ARROW_STEP_RATIO = 0.125;
  const PAGE_STEP_RATIO = 0.875;
  const TEXT_OR_CHOICE = 'input, select, textarea, [contenteditable="true"]';
  const ACTIVATABLE = 'button, a[href]';

  function scrollOffset(key: string, body: HTMLElement): number | null {
    switch (key) {
      case 'ArrowDown':
        return body.clientHeight * ARROW_STEP_RATIO;
      case 'ArrowUp':
        return -body.clientHeight * ARROW_STEP_RATIO;
      case 'PageDown':
      case ' ':
        return body.clientHeight * PAGE_STEP_RATIO;
      case 'PageUp':
        return -body.clientHeight * PAGE_STEP_RATIO;
      case 'Home':
        return -body.scrollTop;
      case 'End':
        return body.scrollHeight - body.scrollTop;
      default:
        return null;
    }
  }

  function scrollWithKeyboard(event: KeyboardEvent): void {
    const body = bodyElement;
    if (body === null || event.defaultPrevented || event.altKey || event.metaKey || event.ctrlKey) {
      return;
    }
    if (event.target instanceof Element) {
      if (event.target.closest(TEXT_OR_CHOICE) !== null) {
        return;
      }
      if (event.key === ' ' && event.target.closest(ACTIVATABLE) !== null) {
        return;
      }
    }
    const offset = scrollOffset(event.key, body);
    if (offset === null) {
      return;
    }
    event.preventDefault();
    body.scrollBy({ top: offset });
  }

  $effect(() => {
    const parts = [headerElement, contentElement, footerElement].filter(
      (element): element is HTMLElement => element !== null,
    );
    if (parts.length === 0) {
      return;
    }
    const report = () => {
      onheightchange(
        parts.reduce((sum, element) => sum + element.getBoundingClientRect().height, 0),
      );
    };
    const observer = new ResizeObserver(report);
    for (const element of parts) {
      observer.observe(element);
    }
    return () => observer.disconnect();
  });
</script>

<svelte:window onkeydown={scrollWithKeyboard} />

<div class="shell">
  {#if header}
    <header class="header" bind:this={headerElement}>
      {@render header()}
    </header>
  {/if}
  <main class="body" aria-busy={busy} bind:this={bodyElement}>
    <div class="content" bind:this={contentElement}>
      {#if notice}
        <p class="notice" role="alert">{notice}</p>
      {/if}
      {@render children()}
    </div>
  </main>
  {#if footer}
    <footer class="footer" bind:this={footerElement}>
      {@render footer()}
    </footer>
  {/if}
</div>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    border-radius: var(--radius-lg);
    background: var(--color-bg-window);
  }

  .header {
    flex: none;
    padding: var(--space-6) var(--space-6) var(--space-4);
    border-bottom: var(--border-width) solid var(--color-border-subtle);
  }

  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-x: hidden;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-4) var(--space-6);
  }

  .footer {
    flex: none;
    padding: var(--space-2) var(--space-4);
    border-top: var(--border-width) solid var(--color-border-subtle);
  }

  .notice {
    padding: var(--space-3) var(--space-5);
    border-radius: var(--radius-md);
    background: var(--color-surface-critical);
    color: var(--color-text-badge-critical);
    font-size: var(--font-size-sm);
  }
</style>
