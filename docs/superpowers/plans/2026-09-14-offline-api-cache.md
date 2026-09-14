# Offline API Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The service worker actually caches the data a household reads offline, never caches session, admin or export traffic, never shows one user another's data, and an app started offline opens as the last signed-in user.

**Architecture:** Workbox runtime routes use self-contained function matchers on same-origin paths, defined in `frontend/src/lib/sw-routes.ts` and imported by `vite.config.ts`. A remembered session profile in `localStorage` lets `loadSession` open the shell in offline mode; a stored cache-owner id clears every cache when the user changes.

**Tech Stack:** Svelte 5 runes, TypeScript, vite-plugin-pwa (generateSW) / Workbox, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-14-offline-api-cache-design.md`

## Global Constraints

- Work on branch `build-offline-cache` created from `main`; never commit to `main`. Before every commit `git branch --show-current` must print `build-offline-cache`.
- vite-plugin-pwa's `generateSW` serialises each `urlPattern` function with `Function.prototype.toString()` into the generated service worker. A matcher must therefore be **self-contained**: no references to imports, module constants or other functions — every path list is written inline inside the function body. A test asserts this by re-creating each matcher from its source text (`new Function('return ' + fn.toString())()`) and running the same cases against the copy.
- Rules, first match wins: (1) `NetworkOnly` for `/api/auth/`, `/api/export`, `/api/import`, `/api/sync/`, `/api/search`, `/api/settings`, `/api/users`, `/api/database`, `/api/tokens`, `/api/me/`, `/api/stats`, `/api/health`; (2) `CacheFirst` `logb-files` (300 entries, 30 days, status 200) for `/api/files/`; (3) `NetworkFirst` `logb-api` (4 s timeout, 200 entries, 7 days, status 200) for `/api/objects`, `/api/activities`, `/api/reminders`, `/api/types`, `/api/tags`; (4) `NetworkOnly` for any other same-origin `/api/`. All with `method: 'GET'`.
- localStorage keys: `logb.cache.user` (id whose data the caches hold), `logb.session.profile` (`{ id, username, is_admin, lang }`). Every access in try/catch.
- Offline mode never sets `sessionKnown = true` and never lets the outbox send; the real session check decides.
- Every UI string in en and de.
- Playwright with project defaults (no `--workers`); foreground commands (timeouts up to 600000 ms); never end a turn while one runs. Never weaken an assertion. Comments explain *why*.
- Commits end with exactly:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DNUZLftSTtND7ABvN6eoGv
  ```

---

### Task 1: Route matchers that match

**Files:**
- Create: `frontend/src/lib/sw-routes.ts`, `frontend/tests/sw-routes.test.ts`
- Modify: `frontend/vite.config.ts`, `frontend/tsconfig.node.json` (include `src/lib/sw-routes.ts` if the node config type-checks `vite.config.ts` and needs it)

**Interfaces:**
- Produces:
  ```ts
  export type RouteMatch = (ctx: { url: URL; sameOrigin: boolean; request?: Request }) => boolean;
  export const neverCached: RouteMatch;      // rule 1
  export const files: RouteMatch;            // rule 2
  export const householdData: RouteMatch;    // rule 3
  export const otherApi: RouteMatch;         // rule 4
  ```

- [ ] **Step 1: Failing tests** — `frontend/tests/sw-routes.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { files, householdData, neverCached, otherApi, type RouteMatch } from '../src/lib/sw-routes';

const at = (path: string, sameOrigin = true) => ({ url: new URL(`https://logb.example${path}`), sameOrigin });

/** The first rule that matches, in the order vite.config.ts registers them. */
function ruleFor(path: string, sameOrigin = true, rules: RouteMatch[] = [neverCached, files, householdData, otherApi]): number {
  return rules.findIndex((r) => r(at(path, sameOrigin)));
}

const CASES: Array<[string, number]> = [
  ['/api/auth/me', 0], ['/api/auth/status', 0], ['/api/auth/login', 0],
  ['/api/export', 0], ['/api/export?object_id=3', 0], ['/api/import', 0],
  ['/api/sync/pull?since=0', 0], ['/api/search?q=golf', 0], ['/api/settings', 0],
  ['/api/users', 0], ['/api/database', 0], ['/api/tokens', 0], ['/api/me/notifications', 0],
  ['/api/stats?year=2026', 0], ['/api/health', 0],
  ['/api/files/12', 1], ['/api/files/12?thumb=1', 1],
  ['/api/objects', 2], ['/api/objects?all=true&archived=false', 2], ['/api/objects/7', 2],
  ['/api/objects/7/insights', 2], ['/api/objects/7/activities?limit=20', 2], ['/api/activities/9', 2],
  ['/api/reminders/due?within_days=30', 2], ['/api/types', 2], ['/api/tags', 2],
  ['/api/something-new', 3],
];

describe('service worker routes', () => {
  it.each(CASES)('%s goes to rule %i', (path, rule) => {
    expect(ruleFor(path)).toBe(rule);
  });

  it('matches nothing on another origin', () => {
    for (const [path] of CASES) expect(ruleFor(path, false)).toBe(-1);
  });

  it('matches nothing outside /api', () => {
    expect(ruleFor('/objects/7')).toBe(-1);
    expect(ruleFor('/apiary')).toBe(-1);
  });

  it('is self-contained, because the service worker receives each matcher as source text', () => {
    const copies = [neverCached, files, householdData, otherApi].map(
      (fn) => new Function(`return (${fn.toString()})`)() as RouteMatch,
    );
    for (const [path, rule] of CASES) expect(ruleFor(path, true, copies)).toBe(rule);
  });
});
```

Run `cd frontend && npx vitest run tests/sw-routes.test.ts` → FAIL (module missing).

- [ ] **Step 2: Implement** `frontend/src/lib/sw-routes.ts`:

```ts
/**
 * Which service-worker cache rule a request falls under. Workbox tests a RegExp `urlPattern`
 * against the whole URL (`https://host/api/...`), so the old `/^\/api\//` patterns never matched
 * and nothing was ever cached -- or protected. These match the path on the same origin instead.
 *
 * SELF-CONTAINED ON PURPOSE: vite-plugin-pwa copies each function into the generated service
 * worker with `toString()`, so a matcher may not use anything from outside its own body -- no
 * imports, no shared constants. `tests/sw-routes.test.ts` rebuilds each one from its source to
 * keep that true.
 */
export type RouteMatch = (ctx: { url: URL; sameOrigin: boolean; request?: Request }) => boolean;

/** Session, administration, exports, sync and anything that must never be answered from disk. */
export const neverCached: RouteMatch = ({ url, sameOrigin }) =>
  sameOrigin &&
  ['/api/auth/', '/api/export', '/api/import', '/api/sync/', '/api/search', '/api/settings', '/api/users',
    '/api/database', '/api/tokens', '/api/me/', '/api/stats', '/api/health']
    .some((prefix) => url.pathname === prefix.replace(/\/$/, '') || url.pathname.startsWith(prefix));

/** Content-addressed blobs: never change under an id. */
export const files: RouteMatch = ({ url, sameOrigin }) => sameOrigin && url.pathname.startsWith('/api/files/');

/** What a household reads with no connection: objects, entries, reminders, types, tags. */
export const householdData: RouteMatch = ({ url, sameOrigin }) =>
  sameOrigin &&
  ['/api/objects', '/api/activities', '/api/reminders', '/api/types', '/api/tags']
    .some((prefix) => url.pathname === prefix || url.pathname.startsWith(`${prefix}/`) || url.pathname.startsWith(`${prefix}?`));

/** Anything else under /api: network only, so a new endpoint is never cached by accident. */
export const otherApi: RouteMatch = ({ url, sameOrigin }) => sameOrigin && url.pathname.startsWith('/api/');
```

Check the `neverCached` prefix logic against the cases (`/api/export?object_id=3` has pathname `/api/export`; `/api/exports` must not exist — fine). Adjust only the implementation until all cases pass.

In `frontend/vite.config.ts`, import the four matchers and replace `runtimeCaching` with (keep comments, updated):

```ts
        runtimeCaching: [
          { urlPattern: neverCached, handler: 'NetworkOnly', method: 'GET' },
          {
            urlPattern: files, handler: 'CacheFirst', method: 'GET',
            options: { cacheName: 'logb-files', expiration: { maxEntries: 300, maxAgeSeconds: 60 * 60 * 24 * 30 }, cacheableResponse: { statuses: [200] } },
          },
          {
            urlPattern: householdData, handler: 'NetworkFirst', method: 'GET',
            options: { cacheName: 'logb-api', networkTimeoutSeconds: 4, expiration: { maxEntries: 200, maxAgeSeconds: 60 * 60 * 24 * 7 }, cacheableResponse: { statuses: [200] } },
          },
          { urlPattern: otherApi, handler: 'NetworkOnly', method: 'GET' },
        ],
```

- [ ] **Step 3: Verify the generated service worker.** `cd frontend && npm run build`, then `grep -n "api/objects\|api/auth" dist/sw.js` (or the generated SW file name from `dist/`): the matcher bodies appear with their path lists inline. Record the snippet in the report.

- [ ] **Step 4:** `cd frontend && npx vitest run && npm run check` → all pass, 0 errors.

- [ ] **Step 5: Commit** — `fix: service worker cache routes match request paths`.

---

### Task 2: One user's caches, and opening offline as the last user

**Files:**
- Create: `frontend/src/lib/cache-owner.ts`, `frontend/tests/cache-owner.test.ts`
- Modify: `frontend/src/stores/session.ts`, `frontend/src/lib/object-cache.ts`, `frontend/src/lib/type-registry.ts` (comments only, plus clearing hook if needed), `frontend/src/lib/api.ts` (flush gate if the outbox lives there), `frontend/src/lib/TopBar.svelte` (offline note), `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`, `docs/superpowers/specs/2026-09-14-own-types-design.md` ("Offline" bullet)

**Interfaces:**
- ```ts
  // cache-owner.ts (pure, storage injected for tests)
  export interface Profile { id: number; username: string; is_admin: boolean; lang: string }
  export function rememberProfile(p: Profile, storage?: Storage): void;
  export function rememberedProfile(storage?: Storage): Profile | null;   // validates shape
  export function forgetProfile(storage?: Storage): void;
  /** True when the caches belong to someone else (or to nobody recorded) and must be cleared; records `userId` as the owner. */
  export function claimCaches(userId: number, storage?: Storage): boolean;
  export function forgetCacheOwner(storage?: Storage): void;
  // session.ts
  export const offline: Readable<boolean>; // true while in offline mode
  ```
- Strings: `nav.offline-mode` ("Offline — showing saved data" / "Offline – gespeicherte Daten").

- [ ] **Step 1: Failing Vitest** — `cache-owner.test.ts` with an in-memory `Storage` stub: remember/read/forget profile (malformed JSON → null; missing fields → null); `claimCaches` returns true for no owner, false for the same id, true for a different id, and records the id; storage that throws (private mode) makes remember a no-op and `rememberedProfile` null and `claimCaches` true (clear to be safe).

- [ ] **Step 2: Implement `cache-owner.ts`** with keys from Global Constraints.

- [ ] **Step 3: Wire `session.ts`.** Read the whole file first.
  - A helper `adoptUser(me)` used by `doLoadSession` (after `/auth/me` succeeds) and `login`: if `claimCaches(me.id)` is true, call `clearObjectCache()` and `clearCustomTypes()` **before** `user.set(me)` and any data load; then `rememberProfile(me)`; set `offline` false.
  - `logout`, `logoutEverywhere` and the unauthorized handler additionally call `forgetProfile()` and `forgetCacheOwner()`.
  - `doLoadSession`: when `/auth/status` (or `/auth/me`) fails **without** a server answer (a connectivity failure, as `isRejection` distinguishes today) and `rememberedProfile()` exists, set `user` to that profile (fill other `User` fields with safe defaults so the type is satisfied), `setOutboxUser(profile.id)`, set `offline` true, and return `false` (session still unknown, so the existing retry listeners keep trying). Do not set `sessionKnown`.
  - Outbox: find where `flushOutbox` decides it may send (it checks for a user today). Add a guard so it also requires the session to be known (export a `sessionIsKnown()` getter from `session.ts` or pass a predicate the way `setOutboxUser` is wired — follow the existing dependency direction and avoid an import cycle). When the retry's real session check succeeds, the existing flush-after-session path sends.
  - When the real check later succeeds for the same user: `offline` false, nothing cleared. For a different user: `adoptUser` clears first. A 401: unauthorized handler as today plus the forgets.

- [ ] **Step 4: TopBar note.** When `$offline`, show a small muted note `$t('nav.offline-mode')` in the top bar (phone and desktop), `role="status"`.

- [ ] **Step 5: Comments and spec.** Correct the comments in `object-cache.ts` (the invariant block mentioning Workbox caches is now accurate — keep it, add the cache-owner guard) and `type-registry.ts` (what is persisted where). Own-types spec "Offline" bullet: the type list is kept per user in local storage and API reads for types are also in the service-worker cache.

- [ ] **Step 6:** `cd frontend && npx vitest run && npm run check`.

- [ ] **Step 7: Commit** — `feat: caches follow the signed-in user, and the app opens offline as the last one`.

---

### Task 3: Offline end-to-end

**Files:**
- Create: `frontend/tests-e2e/25-offline-cache.spec.ts`
- Modify: `docs/superpowers/specs/2026-09-14-offline-api-cache-design.md` (status), `README.md` (one sentence under "On a phone" or wherever offline behaviour is described, if it claims otherwise)

- [ ] **Step 1: Write the spec** (read `04-offline.spec.ts` and `19-offline-edit.spec.ts` first for the local idioms around the service worker and offline):

```ts
import { test, expect, type Page, type BrowserContext } from '@playwright/test';
import { signInFresh } from './helpers';

/** The service worker must control the page before an offline reload can be served from it. */
async function underServiceWorker(page: Page) {
  await page.evaluate(async () => { await navigator.serviceWorker.ready; });
  if (!(await page.evaluate(() => !!navigator.serviceWorker.controller))) await page.reload();
  await expect.poll(() => page.evaluate(() => !!navigator.serviceWorker.controller)).toBe(true);
}

async function object(page: Page, name: string): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { name, type: 'bike', description: '' } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id;
}

test('an app started offline opens as the last user and shows saved objects', async ({ page, context }) => {
  await signInFresh(page, '25-offline-open');
  const id = await object(page, 'Offline Bike');
  await page.goto('/');
  await underServiceWorker(page);
  await expect(page.getByText('Offline Bike')).toBeVisible();
  await page.goto(`/objects/${id}`);
  await expect(page.getByRole('heading', { name: 'Offline Bike' })).toBeVisible();

  await context.setOffline(true);
  await page.goto('/');
  await expect(page.getByText(/Offline/)).toBeVisible();
  await expect(page.getByText('Offline Bike')).toBeVisible();
  await page.goto(`/objects/${id}`);
  await expect(page.getByRole('heading', { name: 'Offline Bike' })).toBeVisible();
  await context.setOffline(false);
});

test('another user never sees the previous user\'s saved objects', async ({ page, context }) => {
  await signInFresh(page, '25-offline-a');
  await object(page, 'Private Of A');
  await page.goto('/');
  await underServiceWorker(page);
  await expect(page.getByText('Private Of A')).toBeVisible();

  // B signs in on the same device (signInFresh signs the admin in and out first).
  await signInFresh(page, '25-offline-b');
  await page.goto('/');
  await expect(page.getByText('Private Of A')).toHaveCount(0);
  await context.setOffline(true);
  await page.goto('/');
  await expect(page.getByText('Private Of A')).toHaveCount(0);
  await context.setOffline(false);
});

test('after signing out, an offline start shows the sign-in screen, not the old session', async ({ page, context }) => {
  await signInFresh(page, '25-offline-out');
  await page.goto('/');
  await underServiceWorker(page);
  await page.getByRole('button', { name: /^E$|avatar|account/i }).first().click().catch(() => {});
  // Sign out through the API-backed UI path used elsewhere in the suite; adjust to the real control.
  await page.request.post('/api/auth/logout');
  await page.evaluate(() => { localStorage.removeItem('logb.session.profile'); });
  await context.setOffline(true);
  await page.goto('/');
  await expect(page.getByText(/My objects|Meine Objekte/)).toHaveCount(0);
  await context.setOffline(false);
});
```

The third test must sign out **through the app's own sign-out control** (find how `13-shell.spec.ts` or `01-smoke.spec.ts` signs out and do the same) instead of the API call and manual `localStorage` removal shown as a placeholder above — the point is to prove the app's sign-out removes the profile. Replace those three lines accordingly; the assertions stay.

If the e2e build does not register a service worker (check `playwright.config.ts` / how the server serves `dist/`), report NEEDS_CONTEXT with what you found instead of weakening the tests.

- [ ] **Step 2: Run** `cd frontend && npm run e2e -- 25-offline-cache` — expected to pass on top of Tasks 1–2 (if it fails, fix the implementation, not the assertions). Then `npm run e2e -- 04-offline 19-offline-edit 13-shell 01-smoke`, then the full suite.

- [ ] **Step 3:** Spec status `implemented`; README sentence if needed.

- [ ] **Step 4: Commit** — `test: the app opens offline as the last user, and never shows another user's saved data`.
