<script lang="ts">
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import type { HTMLSelectAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils.js';
  import { controlClass } from '../field/classes.js';
  import { getFieldContext } from '../field/context.js';

  let { ref = $bindable(null), value = $bindable(), id, class: className, children, ...restProps }:
    HTMLSelectAttributes & { ref?: HTMLSelectElement | null } = $props();
  const field = getFieldContext();
</script>

<!-- A real <select>, styled: the phone's own picker, typeahead through a long list, and
     `selectOption` in the e2e suite all keep working. Only the box and the chevron are ours. -->
<div data-slot="native-select-wrapper" class={cn('relative min-w-0', className)}>
  <select
    bind:this={ref}
    bind:value
    data-slot="native-select"
    id={id ?? field?.id}
    aria-describedby={field?.describedBy}
    aria-invalid={field?.invalid || undefined}
    class={cn(controlClass, 'cursor-pointer appearance-none pr-10')}
    {...restProps}
  >
    {@render children?.()}
  </select>
  <ChevronDown aria-hidden="true" class="pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2 text-muted-foreground" />
</div>
