<script lang="ts">
  import { api } from '../lib/api';
  import { go } from '../lib/router';
  import Logo from '../lib/Logo.svelte';
  import { t } from '../i18n';
  import { loadSession } from '../stores/session';
  let username = $state('');
  let password = $state('');
  let error = $state('');
  let busy = $state(false);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    busy = true; error = '';
    try {
      // The browser's timezone becomes the instance's, unless the server was given one: the
      // person setting it up is almost always sitting where it will be used.
      const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
      await api('POST', '/auth/setup', { username, password, timezone });
      // The admin exists now, but the app has to be able to SAY who is signed in before it can
      // show anything: navigating with the session still unreachable lands on a permanent
      // "Loading…" with no error and no way forward. `loadSession` swallows a failed
      // /auth/status on purpose -- it must not claim the user is signed out when it simply
      // cannot tell -- so the answer comes back as a value instead of a throw.
      if (!(await loadSession())) throw new Error($t('setup.not-reachable'));
      go('/', true);
    } catch (err) {
      error = (err as Error).message;
    } finally { busy = false; }
  }
</script>

<main class="auth">
  <Logo />
  <h1>{$t('setup.title')}</h1>
  <p class="muted">{$t('setup.intro')}</p>
  <form onsubmit={submit}>
    <div class="field"><label for="u">{$t('login.username')}</label><input id="u" bind:value={username} autocomplete="username" required minlength="3" /></div>
    <div class="field"><label for="p">{$t('login.password')}</label><input id="p" type="password" bind:value={password} autocomplete="new-password" required minlength="8" /></div>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy}>{$t('setup.submit')}</button>
  </form>
</main>
