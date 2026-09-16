<script lang="ts">
  import { onDestroy } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import { user, logout, logoutEverywhere, signOutErrorMessage } from '../../stores/session';
  import { createPairing } from '../../lib/pairing';

  let ownPass = $state('');
  let message = $state('');
  let error = $state('');

  const pairing = createPairing();
  const pair = pairing.state;
  onDestroy(pairing.stop);

  async function signOut() {
    try { await logout(); } catch (e) { error = signOutErrorMessage(e, $t); }
  }

  async function signOutEverywhere() {
    if (!confirm($t('settings.logout-all-confirm'))) return;
    try { await logoutEverywhere(); } catch (e) { error = signOutErrorMessage(e, $t); }
  }

  async function changeOwnPassword() {
    if (!$user) return;
    try {
      await api('PATCH', `/users/${$user.id}`, { password: ownPass });
      ownPass = ''; message = $t('object.saved');
    } catch (e) { error = (e as Error).message; }
  }

  async function requestPairCode() {
    try { await pairing.request(); } catch (e) { error = (e as Error).message; }
  }
</script>

<main>
  <TopBar title={$t('settings.account')} backTo="/settings" />
  {#if error}<p class="error">{error}</p>{/if}
  {#if message}<p class="muted">{message}</p>{/if}

  <p class="muted">{$user?.username}</p>
  <div class="row">
    <div class="field"><label for="op">{$t('settings.change-password')}</label><input id="op" type="password" bind:value={ownPass} autocomplete="new-password" /></div>
    <button onclick={changeOwnPassword} disabled={ownPass.length < 8}>{$t('nav.save')}</button>
  </div>
  <button class="ghost" onclick={signOut}>{$t('login.logout')}</button>
  <button class="ghost" onclick={signOutEverywhere}>{$t('settings.logout-all')}</button>
  <p class="muted hint">{$t('settings.logout-all-hint')}</p>

  <h2>{$t('account.pair-title')}</h2>
  {#if $pair.phase === 'idle' || $pair.phase === 'expired'}
    <button onclick={requestPairCode}>
      {$t($pair.phase === 'expired' ? 'account.pair-new' : 'account.pair-show')}
    </button>
  {:else}
    <p class="muted hint">{$t('account.pair-hint')}</p>
    <p class="muted" aria-live="polite">{$t('account.pair-expires', { s: $pair.secondsLeft })}</p>
    {#if $pair.phase === 'qr'}
      <!-- Server-generated SVG from the `qrcode` crate (src/domain/pairing.rs), built from the
           pairing URI on the server side -- never user input -- so inlining it is safe. -->
      <div class="qr" role="img" aria-label={$t('account.pair-title')}>{@html $pair.pair?.qr_svg}</div>
      <button class="ghost" onclick={pairing.showCode}>{$t('account.pair-code')}</button>
    {:else}
      <code class="pair-uri">{$pair.pair?.uri}</code>
      <button class="ghost" onclick={pairing.showQr}>{$t('account.pair-show')}</button>
    {/if}
  {/if}
</main>

<style>
  .row > button { flex: none; }
  .qr { max-width: 240px; }
  .qr :global(svg) { width: 100%; height: auto; display: block; }
  .pair-uri { display: block; word-break: break-all; font-size: var(--text-sm); background: var(--surface-2); padding: var(--space-2); border-radius: var(--radius-sm); }
</style>
