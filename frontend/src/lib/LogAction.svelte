<script lang="ts">
  import { Button } from '$lib/components/ui/button/index.js';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import { t } from '../i18n';
  import type { LogOption } from './log-options';

  let { options, onpick, placement = 'fab' }: {
    options: LogOption[]; onpick: (option: LogOption) => void;
    /** `fab`: the floating pill at the bottom of a phone screen. `header`: a regular button in the
     *  desktop page header, beside Edit. */
    placement?: 'fab' | 'header';
  } = $props();

  // A menu of one is a detour: with only activities to log, the button is the action itself.
  const cls = $derived(placement === 'fab' ? 'h-12 rounded-full px-5 text-base font-semibold shadow-lg' : 'min-h-11 px-4 text-sm font-semibold');
</script>

{#if options.length === 1}
  <Button class={cls} onclick={() => onpick(options[0])}>+ {options[0].label}</Button>
{:else}
  <DropdownMenu.Root>
    <DropdownMenu.Trigger>
      {#snippet child({ props })}
        <Button {...props} class={cls} data-testid="log-menu">+ {$t('dash.log')}</Button>
      {/snippet}
    </DropdownMenu.Trigger>
    <!-- Upward from the floating button (a menu opening down would open off the screen), downward
         from the header. -->
    <DropdownMenu.Content side={placement === 'fab' ? 'top' : 'bottom'} align="end" sideOffset={8} class="min-w-48">
      {#each options as option (option.id)}
        <DropdownMenu.Item class="min-h-11 text-base" onSelect={() => onpick(option)}>{option.label}</DropdownMenu.Item>
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Root>
{/if}
