<script lang="ts">
  import { back, go } from './router';
  import { ensureOutboxCounts, outboxCounts, servingSaved } from './api';
  import { t } from '../i18n';
  import Icon, { type IconName } from './Icon.svelte';
  import AccountMenu from './AccountMenu.svelte';
  import { offline } from '../stores/session';
  let { title, subtitle = null, backTo = null, icon = null, children }: {
    title: string; subtitle?: string | null; backTo?: string | null; icon?: IconName | null; children?: import('svelte').Snippet;
  } = $props();

  // Counted once per queue change in ./api-outbox.ts, not read from IndexedDB on every mount.
  $effect(() => { ensureOutboxCounts(); });
  const pending = $derived($outboxCounts.pending);
  const dead = $derived($outboxCounts.dead);
</script>

<header class="topbar">
  {#if backTo !== null}
    <button class="ghost" aria-label={$t('nav.back')} onclick={() => (backTo ? go(backTo) : back())}><Icon name="back" /></button>
  {/if}
  <!-- tabindex -1: focusable by script only, so navigation can move focus here (see
       `focusPageHeading` in ./router.ts) without adding a Tab stop. -->
  <div class="min-w-0 flex-1">
    <h1 tabindex="-1" class="flex items-center gap-2 truncate text-xl font-semibold tracking-tight desk:text-2xl">{#if icon}<Icon name={icon} />{/if}{title}</h1>
    {#if subtitle}<p class="truncate text-sm text-muted-foreground">{subtitle}</p>{/if}
  </div>
  <!-- In offline mode the user on screen is only the last one remembered here, and what shows is
       what was cached -- worth saying, so stale data is not taken for current. `servingSaved`
       covers the other way this happens: online and signed in, but the network took long enough
       that the service worker answered from `logb-api` instead (see `servedFromCache` in ./api.ts). -->
  {#if $offline || $servingSaved}<span class="offline-note muted" role="status">{$t('nav.offline-mode')}</span>{/if}
  {#if pending > 0}<span class="chip pending">{$t('outbox.pending', { n: pending })}</span>{/if}
  {#if dead > 0}<span class="chip dead">{$t('outbox.dead-chip', { n: dead })}</span>{/if}
  {#if children}{@render children()}{/if}
  <AccountMenu />
</header>

<style>
  /* Focus lands here on navigation, for screen readers -- it is not a control, so no ring. */
  h1:focus { outline: none; }
  .offline-note { font-size: var(--text-xs); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; flex: 0 1 auto; }
</style>
