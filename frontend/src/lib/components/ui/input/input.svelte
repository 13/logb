<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils.js';
  import { controlClass } from '../field/classes.js';
  import { getFieldContext } from '../field/context.js';

  let { ref = $bindable(null), value = $bindable(), type = 'text', id, class: className, ...restProps }:
    HTMLInputAttributes & { ref?: HTMLInputElement | null } = $props();
  const field = getFieldContext();
</script>

<!-- A number field has no browser spinner: it is a stepper nobody wants on a phone, and a unit sits where it would be. -->
<input
  bind:this={ref}
  bind:value
  {type}
  data-slot="input"
  id={id ?? field?.id}
  aria-describedby={field?.describedBy}
  aria-invalid={field?.invalid || undefined}
  class={cn(
    controlClass,
    type === 'number' && '[appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none',
    field?.unit && 'pr-14',
    className,
  )}
  {...restProps}
/>
