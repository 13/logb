<script lang="ts">
  import { go } from '../lib/router';
  import { t } from '../i18n';
  import { login } from '../stores/session';
  let username = $state('');
  let password = $state('');
  let error = $state('');
  let busy = $state(false);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    busy = true; error = '';
    try {
      await login(username, password);
      go('/', true);
    } catch {
      error = $t('login.failed');
    } finally { busy = false; }
  }
</script>

<main>
  <h1>{$t('login.title')}</h1>
  <form onsubmit={submit}>
    <div class="field"><label for="u">{$t('login.username')}</label><input id="u" bind:value={username} autocomplete="username" required /></div>
    <div class="field"><label for="p">{$t('login.password')}</label><input id="p" type="password" bind:value={password} autocomplete="current-password" required /></div>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy}>{$t('login.submit')}</button>
  </form>
</main>
