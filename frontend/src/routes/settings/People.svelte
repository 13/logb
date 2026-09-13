<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import { user } from '../../stores/session';
  import type { User } from '../../lib/types';

  let users = $state<User[]>([]);
  let newName = $state('');
  let newPass = $state('');
  let newAdmin = $state(false);
  let error = $state('');

  onMount(async () => {
    await loadUsers();
  });

  async function loadUsers() {
    try { users = await api<User[]>('GET', '/users'); } catch (e) { error = (e as Error).message; }
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
</script>

<main>
  <TopBar title={$t('settings.users')} backTo="/settings" />
  {#if error}<p class="error">{error}</p>{/if}

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
</main>

<style>
  .toggle input { flex: none; width: 20px; height: 20px; }
  .row > button { flex: none; }
  .danger-text { color: var(--danger); }
</style>
