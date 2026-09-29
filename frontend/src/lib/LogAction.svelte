<script lang="ts">
  import { Button } from '$lib/components/ui/button/index.js';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import { t } from '../i18n';
  import type { LogOption } from './log-options';

  let { options, onpick }: { options: LogOption[]; onpick: (option: LogOption) => void } = $props();

  // A menu of one is a detour: with only activities to log, the button is the action itself.
  const fab = 'h-12 rounded-full px-5 text-base font-semibold shadow-lg';
</script>

{#if options.length === 1}
  <Button class={fab} onclick={() => onpick(options[0])}>+ {options[0].label}</Button>
{:else}
  <DropdownMenu.Root>
    <DropdownMenu.Trigger>
      {#snippet child({ props })}
        <Button {...props} class={fab} data-testid="log-menu">+ {$t('dash.log')}</Button>
      {/snippet}
    </DropdownMenu.Trigger>
    <!-- Upward: the button floats at the bottom of the screen, so a menu opening down would
         open off it. -->
    <DropdownMenu.Content side="top" align="end" sideOffset={8} class="min-w-48">
      {#each options as option (option.id)}
        <DropdownMenu.Item class="min-h-11 text-base" onSelect={() => onpick(option)}>{option.label}</DropdownMenu.Item>
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/if}
