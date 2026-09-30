<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '$lib/utils.js';
  import { errorClass, hintClass, labelClass, warnClass } from './classes.js';
  import { setFieldContext } from './context.js';

  /**
   * One form field: label, control, then an optional hint, warning and error, in that order. The
   * control is the child and reads its id, `aria-describedby` and `aria-invalid` from here.
   *
   * `unit` is drawn inside the control at its right edge ("km") instead of in the label, where
   * units used to stack ("Repeat every (counter) (km)"). Screen readers still hear it as part of
   * the label, so the accessible name stays "Due at (km)".
   */
  let { id, label, hint = '', warn = '', error = '', unit = null, class: className, children }: {
    id: string; label: string; hint?: string; warn?: string; error?: string; unit?: string | null;
    class?: string; children: Snippet;
  } = $props();

  const describedBy = $derived(
    [hint && `${id}-hint`, warn && `${id}-warn`, error && `${id}-error`].filter(Boolean).join(' ') || undefined,
  );
  setFieldContext({
    get id() { return id; },
    get describedBy() { return describedBy; },
    get invalid() { return error !== ''; },
    get unit() { return unit; },
  });
</script>

<div data-slot="field" class={cn('flex min-w-0 flex-col gap-1.5', className)}>
  <label for={id} class={labelClass}>{label}{#if unit}<span class="sr-only"> ({unit})</span>{/if}</label>
  {#if unit}
    <div class="relative">
      {@render children()}
      <span aria-hidden="true" class="pointer-events-none absolute inset-y-0 right-3 flex items-center text-sm text-muted-foreground">{unit}</span>
    </div>
  {:else}
    {@render children()}
  {/if}
  {#if hint}<p id={`${id}-hint`} class={hintClass}>{hint}</p>{/if}
  {#if warn}<p id={`${id}-warn`} role="status" class={warnClass}>{warn}</p>{/if}
  {#if error}<p id={`${id}-error`} role="alert" class={errorClass}>{error}</p>{/if}
</div>
