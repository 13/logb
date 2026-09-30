<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { applyTheme, followTheme } from './lib/theme';
  import type { Component } from 'svelte';
  import { path, match, go, focusPageHeading } from './lib/router';
  import { user, setupRequired, loadSession } from './stores/session';
  import { settings } from './stores/settings';
  import { locale, t } from './i18n';
  import Dashboard from './routes/Dashboard.svelte';
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
  // "auto" keeps listening: a system switch to dark while the app is open follows at once.
  $effect(() => followTheme($settings.theme, matchMedia('(prefers-color-scheme: dark)'), (dark) => applyTheme(document, dark)));

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

  // Route table: pattern → page. The dashboard is the first screen after every start, so it is
  // in the main bundle; every other page is its own chunk, loaded the first time it is visited
  // (and all of them precached by the service worker, so offline navigation still works).
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  type Page = Component<any>;
  type Loader = () => Promise<{ default: Page }>;
  const eager = (comp: Page): Loader => () => Promise.resolve({ default: comp });
  const routes: Array<[string, Loader]> = [
    ['/', eager(Dashboard)],
    // Signed out only: their own small chunk, so an ordinary signed-in start does not fetch them
    // to show its first screen (the service worker may still precache them in the background,
    // like every other chunk).
    ['/login', () => import('./routes/Login.svelte')],
    ['/setup', () => import('./routes/Setup.svelte')],
    ['/objects/new', () => import('./routes/ObjectForm.svelte')],
    ['/objects/:id', () => import('./routes/ObjectDetail.svelte')],
    ['/objects/:id/edit', () => import('./routes/ObjectForm.svelte')],
    ['/objects/:id/activities/new', () => import('./routes/ActivityForm.svelte')],
    ['/objects/:id/activities/:aid', () => import('./routes/ActivityForm.svelte')],
    ['/objects/:id/reminders/new', () => import('./routes/ReminderForm.svelte')],
    ['/objects/:id/reminders/:rid', () => import('./routes/ReminderForm.svelte')],
    ['/objects/:id/reading', () => import('./routes/ReadingForm.svelte')],
    ['/search', () => import('./routes/Search.svelte')],
    ['/stats', () => import('./routes/Stats.svelte')],
    ['/settings', () => import('./routes/Settings.svelte')],
    ['/settings/appearance', () => import('./routes/settings/Appearance.svelte')],
    ['/settings/account', () => import('./routes/settings/Account.svelte')],
    ['/settings/api', () => import('./routes/settings/ApiAccess.svelte')],
    ['/settings/data', () => import('./routes/settings/Data.svelte')],
    ['/settings/people', () => import('./routes/settings/People.svelte')],
    ['/settings/database', () => import('./routes/settings/Database.svelte')],
    ['/settings/notifications', () => import('./routes/settings/Notifications.svelte')],
    ['/settings/types', () => import('./routes/settings/Types.svelte')],
  ];
  const current = $derived.by(() => {
    for (const [pattern, load] of routes) {
      const params = match(pattern, $path);
      if (params) return { pattern, load, params };
    }
    return null;
  });

  /** Pages already loaded, by route pattern: a second visit renders in the same tick. */
  const loaded = new Map<string, Page>([['/', Dashboard]]);
  /** The page component for `current`, once its chunk is in hand. */
  let page = $state<{ pattern: string; comp: Page } | null>(null);
  /** Set only when a chunk takes long enough to notice, so a fast load never flashes "Loading…". */
  let slow = $state(false);
  $effect(() => {
    const route = current;
    if (!route) { page = null; return; }
    const known = loaded.get(route.pattern);
    if (known) { page = { pattern: route.pattern, comp: known }; slow = false; return; }
    let live = true;
    const timer = setTimeout(() => { if (live) slow = true; }, 300);
    // A chunk that cannot be fetched (a deploy replaced it while this tab was open, say): a
    // reload asks for the current build's. Once only per tab -- if that did not help, a reload
    // loop would not either, and "Loading…" stays up instead.
    const reloadOnce = () => {
      if (!live) return;
      try {
        if (sessionStorage.getItem('logb.chunk-reload')) return;
        sessionStorage.setItem('logb.chunk-reload', '1');
      } catch { return; }
      location.reload();
    };
    const attempt = () => route.load().then(async (m) => {
      loaded.set(route.pattern, m.default);
      if (!live) return;
      page = { pattern: route.pattern, comp: m.default };
      slow = false;
      try { sessionStorage.removeItem('logb.chunk-reload'); } catch { /* nothing to forget */ }
      // The router moved focus when the address changed, before this page existed.
      await tick();
      if (live) focusPageHeading();
    }).catch(() => {
      if (!live) return;
      slow = true;
      // Offline, a reload would land on the browser's own error page and strand any queued
      // write with no app left to replay it, so "Loading…" stays up until the connection
      // returns. Then the page reloads (not a second import: the bundler's preload helper
      // skips a stylesheet it already tried, so a retried route would render unstyled).
      if (!navigator.onLine) { addEventListener('online', reloadOnce, { once: true }); return; }
      reloadOnce();
    });
    attempt();
    return () => { live = false; clearTimeout(timer); removeEventListener('online', reloadOnce); };
  });

  // Once the first screen is up, the other pages are fetched while nothing else is going on, so
  // the first visit to each one does not wait on its chunk either.
  $effect(() => {
    if (!$user) return;
    // Not sign-in and setup: a signed-in person is sent away from them (the service worker has
    // them for an offline sign-out all the same).
    const warm = () => { for (const [p, load] of routes) if (p !== '/login' && p !== '/setup') void load().catch(() => {}); };
    const idle = (globalThis as { requestIdleCallback?: (fn: () => void) => number }).requestIdleCallback;
    const handle = idle ? idle(warm) : setTimeout(warm, 2000);
    return () => { if (!idle) clearTimeout(handle); };
  });
</script>

{#if $user === undefined}
  <!-- Outside the shell, so the column's width is set here: .app-content > main's (app.tw.css). -->
  <main class="mx-auto w-full max-w-[720px] p-3 desk:max-w-[1100px]"><p class="m-0 text-sm text-muted-foreground">{$t('nav.loading')}</p></main>
{:else if $path === '/setup' || $path === '/login'}
  <!-- The same loader as every page: a failed chunk reloads once, "Loading…" only if slow. -->
  {#if current && page?.pattern === current.pattern}
    {@const Page = page.comp}
    <Page />
  {:else if slow}
    <main class="mx-auto w-full max-w-[720px] p-3 desk:max-w-[1100px]"><p class="m-0 text-sm text-muted-foreground">{$t('nav.loading')}</p></main>
  {/if}
{:else if $user}
  <!-- The shell is for signed-in users. There is nowhere to navigate to before you are signed
       in, and a nav whose every destination bounces off the route guard is worse than no nav. -->
  <div class="app">
    <AppNav />
    <div class="app-content">
      {#if current && page?.pattern === current.pattern}
        {@const Page = page.comp}
        <Page {...current.params} />
      {:else if current}
        {#if slow}<main><p class="m-0 text-sm text-muted-foreground">{$t('nav.loading')}</p></main>{/if}
      {:else}
        <main>
          <p class="m-0 mb-2 text-sm text-muted-foreground">404</p>
          <a data-slot="not-found-home" href="/" class="text-brand-ink underline underline-offset-4" onclick={(e) => { e.preventDefault(); go('/'); }}>{$t('dash.title')}</a>
        </main>
      {/if}
    </div>
  </div>
{/if}
