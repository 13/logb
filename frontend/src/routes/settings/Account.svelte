<script lang="ts">
  import TopBar from '../../lib/TopBar.svelte';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import { user, logout, logoutEverywhere } from '../../stores/session';

  let ownPass = $state('');
  let message = $state('');
  let error = $state('');

  async function signOutEverywhere() {
    if (!confirm($t('settings.logout-all-confirm'))) return;
    try { await logoutEverywhere(); } catch (e) { error = (e as Error).message; }
  }

  async function changeOwnPassword() {
    if (!$user) return;
    try {
      await api('PATCH', `/users/${$user.id}`, { password: ownPass });
      ownPass = ''; message = $t('object.saved');
    } catch (e) { error = (e as Error).message; }
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
  <button class="ghost" onclick={logout}>{$t('login.logout')}</button>
  <button class="ghost" onclick={signOutEverywhere}>{$t('settings.logout-all')}</button>
  <p class="muted hint">{$t('settings.logout-all-hint')}</p>
</main>
