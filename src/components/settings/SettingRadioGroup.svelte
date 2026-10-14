<script lang="ts" generics="T extends string">
  interface Option {
    value: T;
    label: string;
  }

  interface Props {
    legend: string;
    name: string;
    value: T;
    options: readonly Option[];
    onchange: (value: T) => void;
  }

  let { legend, name, value, options, onchange }: Props = $props();
</script>

<fieldset class="group">
  <legend class="legend">{legend}</legend>
  <div class="options">
    {#each options as option (option.value)}
      <label class="option">
        <input
          class="radio"
          type="radio"
          {name}
          value={option.value}
          checked={option.value === value}
          onchange={() => onchange(option.value)}
        />
        <span>{option.label}</span>
      </label>
    {/each}
  </div>
</fieldset>

<style>
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .legend {
    margin-bottom: var(--space-2);
  }

  .options {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
  }

  .option {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
  }

  .radio {
    position: relative;
    flex: none;
    width: var(--radio-size);
    height: var(--radio-size);
    margin: 0;
    border: var(--border-width) solid var(--color-border-strong);
    border-radius: var(--radius-pill);
    background: transparent;
    -webkit-appearance: none;
    appearance: none;
    transition: border-color var(--duration-fast) var(--easing-standard);
  }

  .radio:checked {
    border-color: var(--color-accent);
    background: var(--color-accent);
  }

  .radio:checked::before {
    content: '';
    position: absolute;
    top: 50%;
    left: 50%;
    width: var(--radio-dot-size);
    height: var(--radio-dot-size);
    border-radius: var(--radius-pill);
    background: var(--color-text-on-control);
    transform: translate(-50%, -50%);
  }
</style>
