<script lang="ts">
  import { go } from '../lib/router';
  import Logo from '../lib/Logo.svelte';
  import PasswordInput from '../lib/PasswordInput.svelte';
  import { controlClass, errorClass, labelClass, primaryButtonClass } from '$lib/components/ui/field/classes.js';
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

<!-- Signed out, so no shell: a card in the middle of the screen both ways. `min-h-dvh`, not a
     fixed height, so a phone with the keyboard up scrolls instead of clipping. Plain class
     strings only: this is its own small chunk, and the component library stays out of it. -->
<main class="mx-auto flex min-h-dvh w-full max-w-sm flex-col justify-center px-4 py-8">
  <div class="flex flex-col gap-6 rounded-xl border border-border bg-card p-6 shadow-sm">
    <div class="flex flex-col items-center gap-3 text-center">
      <Logo size={56} />
      <h1 tabindex="-1" class="m-0 text-2xl font-semibold tracking-tight text-foreground focus:outline-none">{$t('login.title')}</h1>
    </div>
    <form data-testid="auth-form" onsubmit={submit} class="m-0 flex flex-col gap-4">
      <div class="flex flex-col gap-1.5">
        <label for="u" class={labelClass}>{$t('login.username')}</label>
        <input id="u" data-slot="auth-input" class={controlClass} bind:value={username} autocomplete="username" autocapitalize="off" spellcheck="false" required />
      </div>
      <div class="flex flex-col gap-1.5">
        <label for="p" class={labelClass}>{$t('login.password')}</label>
        <PasswordInput id="p" bind:value={password} autocomplete="current-password" required />
      </div>
      {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
      <button data-slot="auth-submit" disabled={busy} class={primaryButtonClass}>{$t('login.submit')}</button>
    </form>
  </div>
</main>
