<script lang="ts">
  import Icon from './Icon.svelte';
  import { t } from '../i18n';
  import { logout, signOutErrorMessage, user } from '../stores/session';

  /** `compact` is the sidebar's version: the name and an icon-only sign-out button, since the
   *  sidebar is 240px wide and already says what the app is. */
  let { compact = false }: { compact?: boolean } = $props();

  let busy = $state(false);
  let error = $state('');

  // No confirmation: signing out loses nothing. Writes still queued offline are tagged with
  // this account and replay the next time it signs in (see `userId` in ./outbox.ts).
  async function signOut() {
    busy = true; error = '';
    try { await logout(); }
    catch (e) { error = signOutErrorMessage(e, $t); }
    finally { busy = false; }
  }
</script>

{#if $user}
  <div class="signed-in" class:compact>
    <!-- The initial is decoration; the name beside it is what is read out. -->
    <span class="avatar" aria-hidden="true">{$user.username.slice(0, 1).toUpperCase()}</span>
    <span class="who">
      {#if !compact}<span class="muted as">{$t('account.signed-in-as')}</span>{/if}
      <span class="name"><b>{$user.username}</b>{#if $user.is_admin}<span class="chip">{$t('settings.user-admin')}</span>{/if}</span>
    </span>
    <button class="ghost signout" onclick={signOut} disabled={busy} aria-label={$t('login.logout')} title={$t('login.logout')}>
      <Icon name="logout" />{#if !compact}<span>{$t('login.logout')}</span>{/if}
    </button>
  </div>
  {#if error}<p class="error">{error}</p>{/if}
{/if}

<style>
  .signed-in {
    display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: var(--space-3);
    background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-2) var(--space-2) var(--space-3);
  }
  .compact { background: none; border: none; padding: 0; gap: var(--space-2); }
  .avatar {
    display: grid; place-items: center; width: 36px; height: 36px; border-radius: var(--radius-full);
    background: var(--accent); color: var(--accent-text); font-weight: 700;
  }
  .compact .avatar { width: 28px; height: 28px; font-size: var(--text-sm); }
  .who { display: flex; flex-direction: column; min-width: 0; }
  .as { font-size: var(--text-xs); }
  .name { display: flex; align-items: center; gap: var(--space-2); min-width: 0; }
  .name b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .name .chip { flex: none; }
  .signout { display: inline-flex; align-items: center; gap: var(--space-2); color: var(--muted); }
  .compact .signout { min-width: 44px; justify-content: center; padding: var(--space-2); }
</style>
