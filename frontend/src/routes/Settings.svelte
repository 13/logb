<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import { api, deadOps, discardDeadOp, retryDead, uploadRaw } from '../lib/api';
  import { t } from '../i18n';
  import { LANG_NAMES, SUPPORTED } from '../i18n/detect';
  import { settings } from '../stores/settings';
  import { currency, user, logout, logoutEverywhere } from '../stores/session';
  import type { ImportCounts, User } from '../lib/types';
  import type { QueuedOp } from '../lib/outbox';

  let dead = $state<QueuedOp[]>([]);
  let users = $state<User[]>([]);
  let newName = $state('');
  let newPass = $state('');
  let newAdmin = $state(false);
  let ownPass = $state('');
  let currencyText = $state('');
  let message = $state('');
  let error = $state('');
  let fileEl: HTMLInputElement;

  const isAdmin = $derived($user?.is_admin === true);

  onMount(async () => {
    currencyText = $currency;
    dead = await deadOps();
    if (isAdmin) await loadUsers();
  });

  async function retryOutbox() {
    await retryDead();
    dead = await deadOps();
  }

  /** A permanently-rejected op (e.g. an upload naming an activity that will never exist) can
   *  never succeed no matter how many times "Try again" is pressed -- this is the only way to
   *  make it leave IndexedDB (and, for an upload, release its file bytes) short of the user
   *  clearing site data entirely. */
  async function discardOp(id: string) {
    if (!confirm($t('nav.confirm-delete'))) return;
    await discardDeadOp(id);
    dead = await deadOps();
  }

  async function loadUsers() {
    try { users = await api<User[]>('GET', '/users'); } catch (e) { error = (e as Error).message; }
  }

  async function saveCurrency() {
    try {
      const s = await api<{ currency: string }>('PUT', '/settings', { currency: currencyText.trim().toUpperCase() });
      currency.set(s.currency);
      message = $t('object.saved');
    } catch (e) { error = (e as Error).message; }
  }

  async function addUser() {
    try {
      await api('POST', '/users', { username: newName, password: newPass, is_admin: newAdmin });
      newName = ''; newPass = ''; newAdmin = false;
      await loadUsers();
    } catch (e) { error = (e as Error).message; }
  }

  async function removeUser(u: User) {
    if (!confirm($t('nav.confirm-delete'))) return;
    try { await api('DELETE', `/users/${u.id}`); await loadUsers(); } catch (e) { error = (e as Error).message; }
  }

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

  async function doImport(files: FileList | null) {
    if (!files || files.length === 0) return;
    try {
      const counts = await uploadRaw<ImportCounts>('/import', files[0], 'application/zip');
      message = $t('settings.import-done', counts as unknown as Record<string, number>);
    } catch (e) { error = (e as Error).message; } finally { fileEl.value = ''; }
  }
</script>

<main>
  <TopBar title={$t('settings.title')} backTo="/" />
  {#if error}<p class="error">{error}</p>{/if}
  {#if message}<p class="muted">{message}</p>{/if}

  {#if dead.length > 0}
    <h2>{$t('outbox.failed')}</h2>
    <div class="list">
      {#each dead as op (op.id)}
        <div class="card row">
          <span>
            <b>{String(op.body.title ?? op.kind)}</b>
            <span class="muted">{op.path}</span>
          </span>
          <button class="ghost danger-text" onclick={() => discardOp(op.id)}>{$t('outbox.discard')}</button>
        </div>
      {/each}
    </div>
    <button onclick={retryOutbox}>{$t('outbox.retry')}</button>
  {/if}

  <h2>{$t('settings.language')}</h2>
  <div class="field">
    <select bind:value={$settings.locale} aria-label={$t('settings.language')}>
      <option value="auto">{$t('settings.language-auto')}</option>
      {#each SUPPORTED as l}<option value={l}>{LANG_NAMES[l]}</option>{/each}
    </select>
  </div>

  <h2>{$t('settings.theme')}</h2>
  <div class="field">
    <select bind:value={$settings.theme} aria-label={$t('settings.theme')}>
      <option value="auto">{$t('settings.theme-auto')}</option>
      <option value="light">{$t('settings.theme-light')}</option>
      <option value="dark">{$t('settings.theme-dark')}</option>
    </select>
  </div>

  <h2>{$t('settings.account')}</h2>
  <p class="muted">{$user?.username}</p>
  <div class="row">
    <div class="field"><label for="op">{$t('settings.change-password')}</label><input id="op" type="password" bind:value={ownPass} autocomplete="new-password" /></div>
    <button onclick={changeOwnPassword} disabled={ownPass.length < 8}>{$t('nav.save')}</button>
  </div>
  <button class="ghost" onclick={logout}>{$t('login.logout')}</button>
  <button class="ghost" onclick={signOutEverywhere}>{$t('settings.logout-all')}</button>
  <p class="muted hint">{$t('settings.logout-all-hint')}</p>

  {#if isAdmin}
    <h2>{$t('settings.currency')}</h2>
    <div class="row">
      <div class="field"><input bind:value={currencyText} maxlength="3" aria-label={$t('settings.currency')} /><span class="hint">{$t('settings.currency-hint')}</span></div>
      <button onclick={saveCurrency}>{$t('nav.save')}</button>
    </div>

    <h2>{$t('settings.users')}</h2>
    <div class="list">
      {#each users as u (u.id)}
        <div class="card row">
          <span>{u.username}{#if u.is_admin} · {$t('settings.user-admin')}{/if}</span>
          {#if u.id !== $user?.id}<button class="ghost danger-text" onclick={() => removeUser(u)}>{$t('settings.user-delete')}</button>{/if}
        </div>
      {/each}
    </div>
    <h2>{$t('settings.user-new')}</h2>
    <div class="field"><label for="nu">{$t('login.username')}</label><input id="nu" bind:value={newName} /></div>
    <div class="field"><label for="np">{$t('login.password')}</label><input id="np" type="password" bind:value={newPass} autocomplete="new-password" /></div>
    <label class="row toggle"><input type="checkbox" bind:checked={newAdmin} /> {$t('settings.user-admin')}</label>
    <button class="primary" onclick={addUser} disabled={newName.length < 3 || newPass.length < 8}>{$t('settings.user-new')}</button>
  {/if}

  <h2>{$t('settings.data')}</h2>
  <div class="list">
    <a class="button-like" href="/api/export">{$t('settings.export')}</a>
    <button onclick={() => fileEl.click()}>{$t('settings.import')}</button>
    <input bind:this={fileEl} type="file" accept=".zip,application/zip" hidden onchange={(e) => doImport((e.currentTarget as HTMLInputElement).files)} />
  </div>

  <p class="muted version">{$t('settings.version')} {__APP_VERSION__}</p>
</main>

<style>
  .row > button { flex: none; }
  .toggle input { flex: none; width: 20px; height: 20px; }
  .danger-text { color: var(--danger); }
  .button-like { display: block; text-align: center; padding: 10px 16px; border-radius: var(--radius); background: var(--surface-2); color: var(--text); text-decoration: none; }
  .version { margin-top: 24px; }
</style>
