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

<header class="sticky top-0 z-[5] flex items-center gap-2 bg-background py-2">
  {#if backTo !== null}
    <button data-slot="topbar-back" aria-label={$t('nav.back')} onclick={() => (backTo ? go(backTo) : back())}
            class="grid size-11 shrink-0 cursor-pointer place-items-center rounded-md text-foreground hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring"><Icon name="back" /></button>
  {/if}
  <!-- tabindex -1: focusable by script only (`focusPageHeading` in ./router.ts), no Tab stop and
       no ring -- it is not a control. -->
  <div class="min-w-0 flex-1">
    <h1 tabindex="-1" class="m-0 flex min-w-0 items-center gap-2 text-xl font-semibold tracking-tight focus:outline-none desk:text-2xl">{#if icon}<Icon name={icon} />{/if}<span class="truncate">{title}</span></h1>
    {#if subtitle}<p class="m-0 truncate text-sm text-muted-foreground">{subtitle}</p>{/if}
  </div>
  <!-- In offline mode what shows is what was cached -- worth saying. `servingSaved` covers the
       other way this happens (see `servedFromCache` in ./api.ts). -->
  {#if $offline || $servingSaved}<span role="status" class="min-w-0 shrink truncate text-xs whitespace-nowrap text-muted-foreground">{$t('nav.offline-mode')}</span>{/if}
  {#if pending > 0}<span class="shrink-0 rounded-full bg-warn px-3 py-0.5 text-xs whitespace-nowrap text-background">{$t('outbox.pending', { n: pending })}</span>{/if}
  {#if dead > 0}<span class="shrink-0 rounded-full bg-destructive px-3 py-0.5 text-xs whitespace-nowrap text-destructive-foreground">{$t('outbox.dead-chip', { n: dead })}</span>{/if}
  {#if children}{@render children()}{/if}
  <AccountMenu />
</header>
