<script lang="ts">
  import { api } from '../lib/api';
  import { go } from '../lib/router';
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
      await api('POST', '/auth/setup', { username, password });
      await loadSession();
      go('/', true);
    } catch (err) {
      error = (err as Error).message;
    } finally { busy = false; }
  }
</script>

<main>
  <h1>{$t('setup.title')}</h1>
  <p class="muted">{$t('setup.intro')}</p>
  <form onsubmit={submit}>
    <div class="field"><label for="u">{$t('login.username')}</label><input id="u" bind:value={username} autocomplete="username" required minlength="3" /></div>
    <div class="field"><label for="p">{$t('login.password')}</label><input id="p" type="password" bind:value={password} autocomplete="new-password" required minlength="8" /></div>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy}>{$t('setup.submit')}</button>
  </form>
</main>
