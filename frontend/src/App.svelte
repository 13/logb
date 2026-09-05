<script lang="ts">
  import { onMount } from 'svelte';
  import type { Component } from 'svelte';
  import { path, match, go } from './lib/router';
  import { user, setupRequired, loadSession } from './stores/session';
  import { settings } from './stores/settings';
  import { locale, t } from './i18n';
  import Setup from './routes/Setup.svelte';
  import Login from './routes/Login.svelte';
  import Dashboard from './routes/Dashboard.svelte';

  onMount(() => { loadSession(); });

  $effect(() => { document.documentElement.lang = $locale; });
  $effect(() => {
    const pref = $settings.theme;
    const dark = pref === 'dark' || (pref === 'auto' && matchMedia('(prefers-color-scheme: dark)').matches);
    document.documentElement.dataset.theme = dark ? 'dark' : 'light';
  });

  // Route guards: setup first, then login, then the app.
  $effect(() => {
    if ($user === undefined) return;
    if ($setupRequired && $path !== '/setup') go('/setup', true);
    else if (!$setupRequired && $user === null && $path !== '/login') go('/login', true);
    else if ($user && ($path === '/login' || $path === '/setup')) go('/', true);
  });

  // Route table: pattern → [component, param names]. Later tasks add entries here.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const routes: Array<[string, Component<any>]> = [
    ['/', Dashboard],
  ];
  const current = $derived.by(() => {
    for (const [pattern, comp] of routes) {
      const params = match(pattern, $path);
      if (params) return { comp, params };
    }
    return null;
  });
</script>

{#if $user === undefined}
  <main><p class="muted">{$t('nav.loading')}</p></main>
{:else if $path === '/setup'}
  <Setup />
{:else if $path === '/login'}
  <Login />
{:else if $user && current}
  {@const Page = current.comp}
  <Page {...current.params} />
{:else if $user}
  <main><p class="muted">404</p><a href="/" onclick={(e) => { e.preventDefault(); go('/'); }}>{$t('dash.title')}</a></main>
{/if}
