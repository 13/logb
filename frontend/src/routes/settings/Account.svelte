<script lang="ts">
  import { errorMessage } from '../../lib/api-error';
  import { onDestroy } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import SignedIn from '../../lib/SignedIn.svelte';
  import PasswordInput from '../../lib/PasswordInput.svelte';
  import Toaster from '../../lib/Toaster.svelte';
  import { toast } from '../../lib/toast';
  import { Button } from '$lib/components/ui/button/index.js';
  import { errorClass, hintClass, labelClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import { user, logoutEverywhere, signOutErrorMessage } from '../../stores/session';
  import { createPairing } from '../../lib/pairing';

  let currentPass = $state('');
  let ownPass = $state('');
  let busy = $state(false);
  let passwordError = $state('');
  let error = $state('');

  const pairing = createPairing();
  const pair = pairing.state;
  onDestroy(pairing.stop);

  async function signOutEverywhere() {
    if (!confirm($t('settings.logout-all-confirm'))) return;
    error = '';
    try { await logoutEverywhere(); } catch (e) { error = signOutErrorMessage(e, $t); }
  }

  // The server wants the password being replaced (`current_password`), so a session left open on
  // a shared computer is not enough to take the account over. A wrong one comes back as a 403
  // `wrong_password`, said here in the reader's language.
  async function changeOwnPassword(e: SubmitEvent) {
    e.preventDefault();
    if (!$user) return;
    busy = true; passwordError = '';
    try {
      await api('PATCH', `/users/${$user.id}`, { password: ownPass, current_password: currentPass });
      currentPass = ''; ownPass = '';
      toast($t('settings.password-changed'));
    } catch (err) { passwordError = errorMessage(err, $t); } finally { busy = false; }
  }

  async function requestPairCode() {
    error = '';
    try { await pairing.request(); } catch (e) { error = errorMessage(e, $t); }
  }

  const card = 'flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs';
</script>

<main>
  <TopBar title={$t('settings.account')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
    <!-- Who this is, and "Sign out" (the e2e suite finds it inside `main`). -->
    <SignedIn />

    <form aria-labelledby="account-password" onsubmit={changeOwnPassword} class={`m-0 ${card}`}>
      <h2 id="account-password" class={sectionHeadingClass}>{$t('settings.change-password')}</h2>
      <div class="flex flex-col gap-1.5">
        <label for="cp" class={labelClass}>{$t('settings.current-password')}</label>
        <PasswordInput id="cp" bind:value={currentPass} autocomplete="current-password" required />
      </div>
      <div class="flex flex-col gap-1.5">
        <label for="op" class={labelClass}>{$t('settings.new-password')}</label>
        <PasswordInput id="op" bind:value={ownPass} autocomplete="new-password" required minlength={8} describedby="op-hint" />
        <p id="op-hint" class={hintClass}>{$t('setup.password-hint')}</p>
      </div>
      {#if passwordError}<p role="alert" class={errorClass}>{passwordError}</p>{/if}
      <!-- The page's one primary action. -->
      <Button type="submit" class="h-12 self-start" disabled={busy || ownPass.length < 8 || currentPass.length === 0}>{$t('settings.change-password')}</Button>
    </form>

    <section aria-labelledby="account-sessions" class={card}>
      <h2 id="account-sessions" class={sectionHeadingClass}>{$t('settings.sessions')}</h2>
      <p class={hintClass}>{$t('settings.logout-all-hint')}</p>
      <Button variant="destructive" class="h-12 self-start" onclick={signOutEverywhere}>{$t('settings.logout-all')}</Button>
    </section>

    <section aria-labelledby="account-pair" class={card}>
      <h2 id="account-pair" class={sectionHeadingClass}>{$t('account.pair-title')}</h2>
      {#if $pair.phase === 'idle' || $pair.phase === 'expired'}
        <Button variant="outline" class="h-12 self-start" onclick={requestPairCode}>
          {$t($pair.phase === 'expired' ? 'account.pair-new' : 'account.pair-show')}
        </Button>
      {:else}
        <p class={hintClass}>{$t('account.pair-hint')}</p>
        <p class={hintClass} aria-live="polite">{$t('account.pair-expires', { s: $pair.secondsLeft })}</p>
        {#if $pair.phase === 'qr'}
          <!-- Server-generated SVG (src/domain/pairing.rs) from the pairing URI, never user input. -->
          <div class="w-60 max-w-full [&_svg]:block [&_svg]:h-auto [&_svg]:w-full" role="img" aria-label={$t('account.pair-title')}>{@html $pair.pair?.qr_svg}</div>
          <Button variant="outline" class="min-h-11 self-start" onclick={pairing.showCode}>{$t('account.pair-code')}</Button>
        {:else}
          <code class="block rounded-md bg-muted p-2 font-mono text-sm break-all text-foreground">{$pair.pair?.uri}</code>
          <Button variant="outline" class="min-h-11 self-start" onclick={pairing.showQr}>{$t('account.pair-show')}</Button>
        {/if}
      {/if}
    </section>
  </div>
  <Toaster />
</main>
