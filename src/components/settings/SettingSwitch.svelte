<script lang="ts">
  interface Props {
    label: string;
    hint?: string;
    checked: boolean;
    onchange: (checked: boolean) => void;
  }

  let { label, hint, checked, onchange }: Props = $props();

  const uid = $props.id();

  function handleKeydown(event: KeyboardEvent & { currentTarget: HTMLInputElement }): void {
    if (event.key === 'Enter') {
      event.preventDefault();
      event.currentTarget.click();
    }
  }
</script>

<div class="setting">
  <div class="text">
    <label for={`${uid}-input`}>{label}</label>
    {#if hint}
      <span class="hint" id={`${uid}-hint`}>{hint}</span>
    {/if}
  </div>
  <input
    id={`${uid}-input`}
    class="switch"
    type="checkbox"
    role="switch"
    aria-checked={checked}
    aria-describedby={hint ? `${uid}-hint` : undefined}
    {checked}
    onchange={(event) => onchange(event.currentTarget.checked)}
    onkeydown={handleKeydown}
  />
</div>

<style>
  .setting {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-5);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .hint {
    color: var(--color-text-tertiary);
    font-size: var(--font-size-xs);
  }

  .switch {
    position: relative;
    flex: none;
    width: var(--switch-width);
    height: var(--switch-height);
    margin: 0;
    border-radius: var(--radius-pill);
    background: var(--color-border-strong);
    -webkit-appearance: none;
    appearance: none;
    transition: background-color var(--duration-fast) var(--easing-standard);
  }

  .switch::before {
    content: '';
    position: absolute;
    top: calc((var(--switch-height) - var(--switch-knob-size)) / 2);
    left: calc((var(--switch-height) - var(--switch-knob-size)) / 2);
    width: var(--switch-knob-size);
    height: var(--switch-knob-size);
    border-radius: var(--radius-pill);
    background: var(--color-text-on-control);
    transition: transform var(--duration-fast) var(--easing-standard);
  }

  .switch:checked {
    background: var(--color-accent);
  }

  .switch:checked::before {
    transform: translateX(calc(var(--switch-width) - var(--switch-height)));
  }
</style>
