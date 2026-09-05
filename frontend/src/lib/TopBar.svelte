<script lang="ts">
  import { back, go } from './router';
  import { t } from '../i18n';
  let { title, backTo = null, showSettings = false, children }: { title: string; backTo?: string | null; showSettings?: boolean; children?: import('svelte').Snippet } = $props();
</script>

<header class="topbar">
  {#if backTo !== null}
    <button class="ghost" aria-label={$t('nav.back')} onclick={() => (backTo ? go(backTo) : back())}>←</button>
  {/if}
  <h1>{title}</h1>
  {#if children}{@render children()}{/if}
  {#if showSettings}
    <button class="ghost" aria-label={$t('nav.settings')} onclick={() => go('/settings')}>⚙</button>
  {/if}
</header>
