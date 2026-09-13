<script lang="ts">
  import { back, go } from './router';
  import { onOutboxFlushed, outboxDeadCount, outboxPending } from './api';
  import { t } from '../i18n';
  import Icon, { type IconName } from './Icon.svelte';
  import AccountMenu from './AccountMenu.svelte';
  let { title, backTo = null, icon = null, children }: {
    title: string; backTo?: string | null; icon?: IconName | null; children?: import('svelte').Snippet;
  } = $props();

  let pending = $state(0);
  let dead = $state(0);
  async function refresh() {
    pending = await outboxPending();
    dead = await outboxDeadCount();
  }
  $effect(() => {
    refresh();
    globalThis.addEventListener?.('online', refresh);
    globalThis.addEventListener?.('offline', refresh);
    // `online`/`offline` alone leave this stale on reconnect: api.ts's own `online` listener
    // (which actually flushes the queue) is registered before this one ever runs, so `refresh`
    // above reads the pending count before the flush has removed anything, and nothing then
    // refreshes it again until the component remounts. Subscribing to the flush itself closes
    // that gap regardless of which listener fired first.
    const unsubscribe = onOutboxFlushed(refresh);
    return () => {
      globalThis.removeEventListener?.('online', refresh);
      globalThis.removeEventListener?.('offline', refresh);
      unsubscribe();
    };
  });
</script>

<header class="topbar">
  {#if backTo !== null}
    <button class="ghost" aria-label={$t('nav.back')} onclick={() => (backTo ? go(backTo) : back())}><Icon name="back" /></button>
  {/if}
  <h1>{#if icon}<Icon name={icon} />{/if}{title}</h1>
  {#if pending > 0}<span class="chip pending">{$t('outbox.pending', { n: pending })}</span>{/if}
  {#if dead > 0}<span class="chip dead">{$t('outbox.dead-chip', { n: dead })}</span>{/if}
  {#if children}{@render children()}{/if}
  <AccountMenu />
</header>

<style>
  h1 :global(svg) { vertical-align: -3px; margin-right: var(--space-2); flex: none; }
</style>
