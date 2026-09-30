<script lang="ts" generics="T extends string">
  import { cn } from '$lib/utils.js';
  import { hintClass, labelClass } from '../field/classes.js';

  /**
   * Two or three options side by side. Native radios underneath: the group, one tab stop, the
   * arrow keys, `getByRole('radio')` and `.check()` come from the browser. Each radio is
   * invisible and fills its label, which carries the look; the checked one is amber-tinted with
   * an amber-ink edge (the nav's active pair: >= 4.5:1 text, >= 3:1 edge). The focus ring is
   * drawn outside the label, offset onto the page, so it never sits on the tint.
   */
  let { legend, name, options, value = $bindable(), hint = '', onchange, class: className }: {
    legend: string; name: string; options: ReadonlyArray<{ value: T; label: string }>; value: T;
    hint?: string; onchange?: (value: T) => void; class?: string;
  } = $props();
</script>

<fieldset data-slot="segmented" class={cn('m-0 min-w-0 border-0 p-0', className)} aria-describedby={hint ? `${name}-hint` : undefined}>
  <legend class={cn(labelClass, 'mb-1.5 p-0')}>{legend}</legend>
  <div class="grid auto-cols-fr grid-flow-col gap-2">
    {#each options as option (option.value)}
      <label class="relative flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-input bg-card px-2 py-1.5 text-center text-sm font-medium text-balance text-foreground transition-colors hover:not-has-checked:bg-accent has-checked:border-brand-ink has-checked:bg-primary/10 has-checked:font-semibold has-checked:text-brand-ink has-focus-visible:outline-2 has-focus-visible:outline-solid has-focus-visible:outline-offset-2 has-focus-visible:outline-ring">
        <input type="radio" data-slot="segmented-option" {name} value={option.value} checked={value === option.value}
               onchange={() => { value = option.value; onchange?.(option.value); }}
               class="absolute inset-0 m-0 size-full cursor-pointer appearance-none rounded-lg opacity-0" />
        {option.label}
      </label>
    {/each}
  </div>
  {#if hint}<p id={`${name}-hint`} class={cn(hintClass, 'mt-1.5')}>{hint}</p>{/if}
</fieldset>
