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
  import ObjectForm from './routes/ObjectForm.svelte';
  import ObjectDetail from './routes/ObjectDetail.svelte';
  import ActivityForm from './routes/ActivityForm.svelte';
  import ReminderForm from './routes/ReminderForm.svelte';
  import ReadingForm from './routes/ReadingForm.svelte';
  import Search from './routes/Search.svelte';
  import Stats from './routes/Stats.svelte';
  import Settings from './routes/Settings.svelte';
  import SettingsAppearance from './routes/settings/Appearance.svelte';
  import SettingsAccount from './routes/settings/Account.svelte';
  import SettingsApiAccess from './routes/settings/ApiAccess.svelte';
  import SettingsData from './routes/settings/Data.svelte';
  import SettingsPeople from './routes/settings/People.svelte';
  import SettingsDatabase from './routes/settings/Database.svelte';
  import SettingsNotifications from './routes/settings/Notifications.svelte';
  import SettingsTypes from './routes/settings/Types.svelte';
  import { api } from './lib/api';
  import { loadCustomTypes } from './lib/type-registry';
  import { rememberProfile } from './lib/cache-owner';
  import type { User } from './lib/types';
  import AppNav from './lib/AppNav.svelte';

  onMount(() => { loadSession(); });

  $effect(() => { document.documentElement.lang = $locale; });
  // Every screen that shows a type needs the user's own types, so they load once per signed-in
  // user rather than per screen. Keyed on the id so a language PATCH does not reload them.
  const userId = $derived($user?.id);
  $effect(() => { if (userId !== undefined) void loadCustomTypes(userId); });
  // The daily digest is written on the server, in each person's language -- so the server has
  // to know the language they actually read the app in, which lives in this browser. Only when
  // it differs, so this is one request after a language change and none on an ordinary load.
  $effect(() => {
    const u = $user;
    const l = $locale;
    if (!u || u.lang === l) return;
    api<User>('PATCH', `/users/${u.id}`, { lang: l })
      .then((saved) => {
        // The PATCH can outlive the user it was for (a sign-out, another person signing in):
        // only the same person gets the new language, never the old user written back.
        let updated: User | null = null;
        user.update((cur) => {
          if (!cur || cur.id !== saved.id) return cur;
          updated = { ...cur, lang: saved.lang };
          return updated;
        });
        // Remembered too, or every offline start would open in the old language and PATCH again.
        if (updated) rememberProfile(updated);
      })
      .catch(() => { /* the digest stays in the old language until the next load tries again */ });
  });
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

  // An admin-only page reached by typing its URL. This is a convenience, not the security
  // boundary -- every endpoint behind these two pages is already administrator-only on the
  // server, and stays that way. Without it a non-admin gets a screen of controls that each
  // fail with a 403 one at a time, which reads as the app being broken rather than as the
  // page not being theirs.
  const ADMIN_ONLY = ['/settings/people', '/settings/database'];
  $effect(() => {
    if (!$user) return;
    if (!$user.is_admin && ADMIN_ONLY.includes($path)) go('/settings', true);
  });

  // Route table: pattern → [component, param names]. Later tasks add entries here.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const routes: Array<[string, Component<any>]> = [
    ['/', Dashboard],
    ['/objects/new', ObjectForm],
    ['/objects/:id', ObjectDetail],
    ['/objects/:id/edit', ObjectForm],
    ['/objects/:id/activities/new', ActivityForm],
    ['/objects/:id/activities/:aid', ActivityForm],
    ['/objects/:id/reminders/new', ReminderForm],
    ['/objects/:id/reminders/:rid', ReminderForm],
    ['/objects/:id/reading', ReadingForm],
    ['/search', Search],
    ['/stats', Stats],
    ['/settings', Settings],
    ['/settings/appearance', SettingsAppearance],
    ['/settings/account', SettingsAccount],
    ['/settings/api', SettingsApiAccess],
    ['/settings/data', SettingsData],
    ['/settings/people', SettingsPeople],
    ['/settings/database', SettingsDatabase],
    ['/settings/notifications', SettingsNotifications],
    ['/settings/types', SettingsTypes],
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
{:else if $user}
  <!-- The shell is for signed-in users. There is nowhere to navigate to before you are signed
       in, and a nav whose every destination bounces off the route guard is worse than no nav. -->
  <div class="app">
    <AppNav />
    <div class="app-content">
      {#if current}
        {@const Page = current.comp}
        <Page {...current.params} />
      {:else}
        <main><p class="muted">404</p><a href="/" onclick={(e) => { e.preventDefault(); go('/'); }}>{$t('dash.title')}</a></main>
      {/if}
    </div>
  </div>
{/if}
