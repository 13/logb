<script lang="ts">
  import { Checkbox } from '../checkbox/index.js';
  import { hintClass } from './classes.js';

  /** A checkbox with its label beside it and an optional hint under the label. The label toggles
   *  the box; the box's own hit area reaches 44 px. `detail` follows the label, muted
   *  ("Oil change · every 15,000 km"), and is part of its name. */
  let { id, label, detail = '', hint = '', checked = $bindable(false), disabled = false }: {
    id: string; label: string; detail?: string; hint?: string; checked?: boolean; disabled?: boolean;
  } = $props();
</script>

<div data-slot="check-field" class="flex items-start gap-3">
  <span class="flex h-11 shrink-0 items-center">
    <Checkbox {id} bind:checked {disabled} aria-describedby={hint ? `${id}-hint` : undefined}
              class="relative before:absolute before:-inset-3 before:content-['']" />
  </span>
  <span class="flex min-w-0 flex-col">
    <label for={id} class="flex min-h-11 cursor-pointer items-center text-base text-foreground">
      <span>{label}{#if detail}<span class="text-muted-foreground"> · {detail}</span>{/if}</span>
    </label>
    {#if hint}<p id={`${id}-hint`} class={`${hintClass} -mt-2`}>{hint}</p>{/if}
  </span>
</div>
