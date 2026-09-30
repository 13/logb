<script lang="ts">
  import type { HTMLTextareaAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils.js';
  import { controlClass } from '../field/classes.js';
  import { getFieldContext } from '../field/context.js';

  let { ref = $bindable(null), value = $bindable(), id, class: className, ...restProps }:
    HTMLTextareaAttributes & { ref?: HTMLTextAreaElement | null } = $props();
  const field = getFieldContext();
</script>

<textarea
  bind:this={ref}
  bind:value
  data-slot="textarea"
  id={id ?? field?.id}
  aria-describedby={field?.describedBy}
  aria-invalid={field?.invalid || undefined}
  class={cn(controlClass, 'h-auto min-h-24 resize-y py-2.5', className)}
  {...restProps}
></textarea>
