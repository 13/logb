<script lang="ts">
  import Archive from '@lucide/svelte/icons/archive';
  import Bell from '@lucide/svelte/icons/bell';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import Database from '@lucide/svelte/icons/database';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Palette from '@lucide/svelte/icons/palette';
  import Shapes from '@lucide/svelte/icons/shapes';
  import UserRound from '@lucide/svelte/icons/user-round';
  import UsersRound from '@lucide/svelte/icons/users-round';
  import { go } from './router';
  import { t } from '../i18n';
  import type { SettingsIcon, SettingsRowModel } from './settings-rows';

  let { row }: { row: SettingsRowModel } = $props();
  const ICONS: Record<SettingsIcon, typeof Bell> = {
    palette: Palette, user: UserRound, bell: Bell, shapes: Shapes, key: KeyRound,
    archive: Archive, users: UsersRound, database: Database,
  };
  const Glyph = $derived(ICONS[row.icon]);

  /** A plain left click navigates in place; anything with a modifier (new tab, new window,
   *  download) is left to the browser. Middle clicks never reach `click`. */
  function open(e: MouseEvent) {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    e.preventDefault();
    go(row.path);
  }
</script>

<!-- One row of a grouped list: the icon tile, the name, the current value, a chevron. The whole
     row is a link, so its name is "Account ben", the e2e suite finds it by /Account/, and a
     middle click, Ctrl/Cmd-click or "Open in new tab" opens the page in a new tab. A plain click
     stays in the app (`go`). A row with no value renders no value element at all. -->
<a data-slot="settings-row" href={row.path} onclick={open}
   class="flex min-h-14 w-full cursor-pointer items-center gap-3 px-3 py-2 text-left no-underline transition-colors hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring">
  <span aria-hidden="true" class="grid size-9 shrink-0 place-items-center rounded-md bg-primary/10 text-brand-ink"><Glyph class="size-5" /></span>
  <span class="min-w-0 flex-1 truncate text-base font-medium text-foreground">{$t(row.label)}</span>
  {#if row.value}<span class="min-w-0 max-w-[45%] truncate text-sm text-muted-foreground">{row.value}</span>{/if}
  <ChevronRight aria-hidden="true" class="size-4 shrink-0 text-muted-foreground" />
</a>
