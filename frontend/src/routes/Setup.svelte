<script lang="ts">
  import { errorMessage } from '../lib/api-error';
  import { api } from '../lib/api';
  import { go } from '../lib/router';
  import Logo from '../lib/Logo.svelte';
  import PasswordInput from '../lib/PasswordInput.svelte';
  import { controlClass, errorClass, hintClass, labelClass, primaryButtonClass } from '$lib/components/ui/field/classes.js';
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
      error = errorMessage(err, $t);
    } finally { busy = false; }
  }
</script>

<!-- The first screen of a fresh install: the same card as sign-in. -->
<main class="mx-auto flex min-h-dvh w-full max-w-sm flex-col justify-center px-4 py-8">
  <div class="flex flex-col gap-6 rounded-xl border border-border bg-card p-6 shadow-sm">
    <div class="flex flex-col items-center gap-3 text-center">
      <Logo size={56} />
      <h1 tabindex="-1" class="m-0 text-2xl font-semibold tracking-tight text-foreground focus:outline-none">{$t('setup.title')}</h1>
      <p class="m-0 text-sm text-muted-foreground">{$t('setup.intro')}</p>
    </div>
    <form data-testid="auth-form" onsubmit={submit} class="m-0 flex flex-col gap-4">
      <div class="flex flex-col gap-1.5">
        <label for="u" class={labelClass}>{$t('login.username')}</label>
        <input id="u" data-slot="auth-input" class={controlClass} bind:value={username} autocomplete="username" autocapitalize="off" spellcheck="false"
               required minlength="3" aria-describedby="u-hint" />
        <p id="u-hint" class={hintClass}>{$t('setup.username-hint')}</p>
      </div>
      <div class="flex flex-col gap-1.5">
        <label for="p" class={labelClass}>{$t('login.password')}</label>
        <PasswordInput id="p" bind:value={password} autocomplete="new-password" required minlength={8} describedby="p-hint" />
        <p id="p-hint" class={hintClass}>{$t('setup.password-hint')}</p>
      </div>
      {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
      <button data-slot="auth-submit" disabled={busy} class={primaryButtonClass}>{$t('setup.submit')}</button>
    </form>
  </div>
</main>
