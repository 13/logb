<script lang="ts">
  import { Checkbox as CheckboxPrimitive } from 'bits-ui';
  import CheckIcon from '@lucide/svelte/icons/check';
  import { cn, type WithoutChildrenOrChild } from '$lib/utils.js';

  let { ref = $bindable(null), checked = $bindable(false), class: className, ...restProps }:
    WithoutChildrenOrChild<CheckboxPrimitive.RootProps> = $props();
</script>

<!-- Checked is amber ink with a page-coloured tick, not the amber fill: the box's edge has to
     show against the card (>= 3:1), and amber on white is 2.1:1. The box is 20 px; its hit area
     is widened to 44 px by `before:` where CheckField places it. -->
<CheckboxPrimitive.Root
  bind:ref
  bind:checked
  data-slot="checkbox"
  class={cn(
    'grid size-5 shrink-0 cursor-pointer place-items-center rounded-[5px] border border-input bg-card text-background transition-colors focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:border-brand-ink data-[state=checked]:bg-brand-ink',
    className,
  )}
  {...restProps}
>
  {#snippet children({ checked })}
    {#if checked}<CheckIcon class="size-3.5" strokeWidth={3} aria-hidden="true" />{/if}
  {/snippet}
</CheckboxPrimitive.Root>
