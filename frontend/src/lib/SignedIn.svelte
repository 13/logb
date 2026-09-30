<script lang="ts">
  import Icon from './Icon.svelte';
  import { t } from '../i18n';
  import { logout, signOutErrorMessage, user } from '../stores/session';

  /** `compact` is the sidebar's version: the name and an icon-only sign-out button. `framed`
   *  draws the card's own border; the account menu's popover already is one. */
  let { compact = false, framed = true }: { compact?: boolean; framed?: boolean } = $props();

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
  <div data-testid="signed-in"
       class={['grid grid-cols-[auto_minmax(0,1fr)_auto] items-center', compact ? 'gap-2' : 'gap-3', framed && !compact && 'rounded-lg border border-border bg-card py-2 pr-2 pl-3 shadow-xs']}>
    <!-- The initial is decoration; the name beside it is what is read out. -->
    <span aria-hidden="true" class={['grid shrink-0 place-items-center rounded-full bg-primary font-bold text-primary-foreground', compact ? 'size-7 text-sm' : 'size-9']}>{$user.username.slice(0, 1).toUpperCase()}</span>
    <span class="flex min-w-0 flex-col">
      {#if !compact}<span class="text-xs text-muted-foreground">{$t('account.signed-in-as')}</span>{/if}
      <span class="flex min-w-0 items-center gap-2">
        <b class="truncate font-semibold text-foreground">{$user.username}</b>
        {#if $user.is_admin}<span class="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('settings.user-admin')}</span>{/if}
      </span>
    </span>
    <button data-slot="sign-out" onclick={signOut} disabled={busy} aria-label={$t('login.logout')} title={$t('login.logout')}
            class={['inline-flex min-h-11 min-w-11 cursor-pointer items-center justify-center gap-2 rounded-md text-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:cursor-default disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring', !compact && 'px-3']}>
      <Icon name="logout" />{#if !compact}<span>{$t('login.logout')}</span>{/if}
    </button>
  </div>
  {#if error}<p role="alert" class="m-0 mt-2 text-sm font-medium text-destructive">{error}</p>{/if}
{/if}
