<script lang="ts">
  import { errorMessage } from '../../lib/api-error';
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import PasswordInput from '../../lib/PasswordInput.svelte';
  import { Button } from '$lib/components/ui/button/index.js';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { errorClass, hintClass, labelClass, sectionHeadingClass, destructiveGhostClass } from '$lib/components/ui/field/classes.js';
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
    try { users = await api<User[]>('GET', '/users'); } catch (e) { error = errorMessage(e, $t); }
  }

  async function addUser(e: SubmitEvent) {
    e.preventDefault();
    try {
      await api('POST', '/users', { username: newName, password: newPass, is_admin: newAdmin });
      newName = ''; newPass = ''; newAdmin = false;
      await loadUsers();
    } catch (err) { error = errorMessage(err, $t); }
  }

  async function removeUser(u: User) {
    if (!confirm($t('nav.confirm-delete'))) return;
    try { await api('DELETE', `/users/${u.id}`); await loadUsers(); } catch (e) { error = errorMessage(e, $t); }
  }
</script>

<main>
  <TopBar title={$t('settings.users')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
    <ul role="list" class="m-0 flex list-none flex-col gap-2 p-0">
      {#each users as u (u.id)}
        <li data-testid="user-row" class="flex min-h-14 items-center gap-3 rounded-lg border border-border bg-card px-3 py-2 shadow-xs">
          <span class="min-w-0 flex-1 truncate font-medium text-foreground">{u.username}</span>
          {#if u.is_admin}<span class="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('settings.user-admin')}</span>{/if}
          {#if u.id !== $user?.id}<Button variant="ghost" class={`min-h-11 shrink-0 ${destructiveGhostClass}`} onclick={() => removeUser(u)}>{$t('settings.user-delete')}</Button>{/if}
        </li>
      {/each}
    </ul>

    <form aria-labelledby="people-new" onsubmit={addUser} class="m-0 flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs">
      <h2 id="people-new" class={sectionHeadingClass}>{$t('settings.user-new')}</h2>
      <Field id="nu" label={$t('login.username')}>
        <Input bind:value={newName} autocomplete="off" autocapitalize="off" spellcheck="false" />
      </Field>
      <div class="flex flex-col gap-1.5">
        <label for="np" class={labelClass}>{$t('login.password')}</label>
        <PasswordInput id="np" bind:value={newPass} autocomplete="new-password" describedby="np-hint" />
        <p id="np-hint" class={hintClass}>{$t('setup.password-hint')}</p>
      </div>
      <CheckField id="nu-admin" label={$t('settings.user-admin')} bind:checked={newAdmin} />
      <!-- The page's one primary action. -->
      <Button type="submit" class="h-12 self-start" disabled={newName.length < 3 || newPass.length < 8}>{$t('settings.user-new')}</Button>
    </form>
  </div>
</main>
