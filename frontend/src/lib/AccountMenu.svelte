<script lang="ts">
  import SignedIn from './SignedIn.svelte';
  import { go } from './router';
  import { t } from '../i18n';
  import { user } from '../stores/session';

  let open = $state(false);
  let root = $state<HTMLDivElement | null>(null);

  // Closes on a tap anywhere else and on Escape, the two ways people dismiss a panel like this.
  $effect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => { if (root && !root.contains(e.target as Node)) open = false; };
    const escape = (e: KeyboardEvent) => { if (e.key === 'Escape') open = false; };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('pointerdown', outside);
      document.removeEventListener('keydown', escape);
    };
  });
</script>

<!-- A phone's way to see who is signed in and to sign out, from any screen: the initial sits at
     the end of the top bar, and the panel it opens is the same card the sidebar and Settings
     show. Hidden on desktop, where the sidebar's foot already carries all of it. -->
{#if $user}
  <div class="account-menu" bind:this={root}>
    <button
      class="avatar"
      aria-label={$t('account.menu', { name: $user.username })}
      aria-expanded={open}
      aria-haspopup="true"
      onclick={() => (open = !open)}
    >{$user.username.slice(0, 1).toUpperCase()}</button>
    {#if open}
      <div class="panel" role="group" aria-label={$t('account.menu', { name: $user.username })}>
        <SignedIn />
        <button class="ghost settings" onclick={() => { open = false; go('/settings/account'); }}>{$t('account.settings')}</button>
      </div>
    {/if}
  </div>
{/if}

<style>
  /* Zero layout height, so the 44px button overhangs the bar's centre line equally above and
     below instead of making the top bar taller: on a screen whose bar holds only a title, a
     taller bar pushed everything under it down and off its spacing. */
  .account-menu { position: relative; flex: none; height: 0; display: flex; align-items: center; }
  /* A 44px tap target (the floor for a touch control) around a 36px circle. The circle is the
     button's background, clipped to its content box by the transparent padding. */
  .avatar {
    width: 44px; height: 44px; min-width: 44px; min-height: 44px; padding: var(--space-1);
    border-radius: var(--radius-full); background: var(--accent); background-clip: content-box;
    color: var(--accent-text); font-weight: 700; font-size: var(--text-sm);
  }
  .panel {
    /* Measured from the zero-height wrapper's centre line: half the button, plus a step. */
    position: absolute; right: 0; top: calc(22px + var(--space-1)); z-index: 20;
    width: min(320px, calc(100vw - 2 * var(--space-3)));
    display: flex; flex-direction: column; gap: var(--space-2);
    background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-md);
    padding: var(--space-2); box-shadow: 0 8px 24px rgba(0, 0, 0, .18);
  }
  .panel :global(.signed-in) { border: none; padding: var(--space-1); }
  .settings { width: 100%; text-align: left; }
  @media (width >= 900px) {
    .account-menu { display: none; }
  }
</style>
