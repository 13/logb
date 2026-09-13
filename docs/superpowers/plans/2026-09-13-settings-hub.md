# Settings Hub Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `/settings` from eleven flat sections in one 453-line scroll into a hub of six small pages, each row carrying its current value.

**Architecture:** Six new route components under `frontend/src/routes/settings/`, each owning one concern lifted verbatim out of today's `Settings.svelte`. `Settings.svelte` itself becomes the hub: a pure row model decides what to show and what value each row carries, and one presentational `SettingsRow` renders them. The extractions happen first, one pair of pages per task, with the old sections left in place until the hub replaces them — so the app keeps working at every commit.

**Tech Stack:** Svelte 5 runes, Vite, vitest, Playwright. No new dependencies.

## Global Constraints

- **No new dependencies.** Not one.
- **No new design tokens.** Use the existing `--bg`, `--surface`, `--surface-2`, `--border`, `--accent`, `--accent-text`, `--muted`, `--danger`, `--warn`, `--space-1..6`, `--radius-sm/md/full`, `--control: 48px`, `--navbar: 56px`, `--text-xs/sm/base/lg/xl`.
- **This is a reorganisation, not a rewrite of behaviour.** Every endpoint, payload, confirmation and guard stays exactly as it is. Extracted code moves verbatim; it is not "improved" in transit.
- **Every destructive action keeps its `confirm()` and its existing `danger` treatment**: sign out everywhere, revoke a token, delete a user, switch the database, restart.
- **The desktop breakpoint is `(width >= 900px)`**, matching the shell. Never `min-width: 900px` — the app deliberately uses range syntax so the two queries cannot both fail at a fractional viewport width.
- **Admin-only pages are `/settings/people` and `/settings/database`.** A non-admin reaching either by URL is redirected to `/settings`. This is a convenience, not the security boundary — the endpoints behind them are already admin-only server-side and stay that way.
- **Icons are real SVG.** `frontend/tests/icons.test.ts` fails the build on emoji or fullwidth-glyph stand-ins, including `←`, `⚙` and `＋`.
- **`Icon.svelte`'s `{:else}{(name satisfies never)}{/if}`** makes the icon list exhaustive at compile time: a name added to `IconName` without a matching branch is a type error, and vice versa.
- **Both themes must work**, light and dark.
- **`npm run check` stays at 0 errors and 0 warnings.**
- Backend is untouched by this entire plan. No file outside `frontend/` changes.

**Baselines before Task 1:** vitest 186, Playwright 78 (39 mobile + 39 desktop), `npm run check` 0/0, build clean.

---

### Task 1: Seven icons

**Files:**
- Modify: `frontend/src/lib/Icon.svelte`

**Interfaces:**
- Produces: the `IconName` union gains `'palette' | 'person' | 'key' | 'box' | 'people' | 'database' | 'chevron'`.

- [ ] **Step 1: Extend the union**

In `frontend/src/lib/Icon.svelte`, the `IconName` type currently ends `… | 'body' | 'object';`. Add a fourth line so it reads:

```ts
  export type IconName = 'back' | 'settings' | 'search' | 'document' | 'camera' | 'edit' | 'repeat'
    | 'plus'
    | 'car' | 'e-bike' | 'bike' | 'motorcycle' | 'home' | 'appliance' | 'tool' | 'body' | 'object'
    | 'palette' | 'person' | 'key' | 'box' | 'people' | 'database' | 'chevron';
```

- [ ] **Step 2: Verify it fails to compile**

Run: `cd frontend && npm run check`

Expected: errors from `Icon.svelte` — the `{(name satisfies never)}` fallback now receives the seven names that have no branch, so `satisfies never` fails. This is the exhaustiveness guard doing its job; it is why the union and the branches cannot drift apart.

- [ ] **Step 3: Add the seven branches**

In the same file, insert these before the final `{:else}` branch, keeping the existing style — 24×24 viewBox, stroke-based, no `fill`, geometry only:

```svelte
  {:else if name === 'palette'}
    <circle cx="13.5" cy="6.5" r="1.25" />
    <circle cx="17.5" cy="10.5" r="1.25" />
    <circle cx="6.5" cy="12.5" r="1.25" />
    <circle cx="8.5" cy="7.5" r="1.25" />
    <path d="M12 2a10 10 0 1 0 0 20 2 2 0 0 0 1.5-3.3 2 2 0 0 1 1.5-3.2h2.5A4.5 4.5 0 0 0 22 11c0-4.97-4.48-9-10-9z" />
  {:else if name === 'person'}
    <circle cx="12" cy="8" r="3.5" />
    <path d="M4.5 20a7.5 7.5 0 0 1 15 0" />
  {:else if name === 'key'}
    <circle cx="7.5" cy="15.5" r="3.5" />
    <path d="M10 13 20 3" />
    <path d="M17 6l2.5 2.5" />
  {:else if name === 'box'}
    <path d="M3 8l9-5 9 5v8l-9 5-9-5z" />
    <path d="M3 8l9 5 9-5" />
    <path d="M12 13v8" />
  {:else if name === 'people'}
    <circle cx="9" cy="8" r="3.2" />
    <path d="M2.5 20a6.5 6.5 0 0 1 13 0" />
    <path d="M16 5.2a3.2 3.2 0 0 1 0 5.6" />
    <path d="M17.5 14.3A6.5 6.5 0 0 1 21.5 20" />
  {:else if name === 'database'}
    <ellipse cx="12" cy="5.5" rx="8" ry="3" />
    <path d="M4 5.5v13c0 1.66 3.58 3 8 3s8-1.34 8-3v-13" />
    <path d="M4 12c0 1.66 3.58 3 8 3s8-1.34 8-3" />
  {:else if name === 'chevron'}
    <path d="m9 18 6-6-6-6" />
```

`box` deliberately differs from the existing `object` icon (which has an open lid): this one is closed, because it stands for *your data* as a thing you take away, not for an object in the app.

- [ ] **Step 4: Verify**

Run: `cd frontend && npm run check && npx vitest run tests/icons.test.ts`

Expected: check reports 0 errors / 0 warnings; the icon test passes (it walks every `.svelte` file under `src` looking for glyph stand-ins, and real SVG is what it wants).

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/Icon.svelte
git commit -m "feat: seven icons for a settings hub"
```

---

### Task 2: The row model

**Files:**
- Create: `frontend/src/lib/settings-rows.ts`
- Test: `frontend/tests/settings-rows.test.ts`

**Interfaces:**
- Consumes: `IconName` from Task 1.
- Produces:
  - `type SettingsGroup = 'you' | 'instance'`
  - `type SettingsRowModel = { id: string; path: string; icon: IconName; label: string; value: string | null; group: SettingsGroup }` — `label` is an i18n key, `value` is already-rendered text or `null` for "show nothing".
  - `function settingsRows(input: SettingsRowsInput): SettingsRowModel[]`
  - `type SettingsRowsInput = { isAdmin: boolean; username: string | null; themeLabel: string; localeLabel: string; tokenCount: number | null; userCount: number | null; backendLabel: string | null }`

- [ ] **Step 1: Write the failing tests**

Create `frontend/tests/settings-rows.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { settingsRows, type SettingsRowsInput } from '../src/lib/settings-rows';

function input(over: Partial<SettingsRowsInput> = {}): SettingsRowsInput {
  return {
    isAdmin: false, username: 'ben', themeLabel: 'Dark', localeLabel: 'EN',
    tokenCount: 2, userCount: 3, backendLabel: 'PostgreSQL', ...over,
  };
}

describe('settingsRows', () => {
  it('gives an ordinary user four rows, all in the "you" group', () => {
    const rows = settingsRows(input());
    expect(rows.map((r) => r.id)).toEqual(['appearance', 'account', 'api', 'data']);
    expect(rows.every((r) => r.group === 'you')).toBe(true);
  });

  it('adds the instance group for an administrator, after the personal rows', () => {
    const rows = settingsRows(input({ isAdmin: true }));
    expect(rows.map((r) => r.id)).toEqual(['appearance', 'account', 'api', 'data', 'people', 'database']);
    expect(rows.filter((r) => r.group === 'instance').map((r) => r.id)).toEqual(['people', 'database']);
  });

  it('carries the current value on each row, which is the point of the hub', () => {
    const rows = settingsRows(input({ isAdmin: true }));
    const value = (id: string) => rows.find((r) => r.id === id)?.value;
    expect(value('appearance')).toBe('Dark · EN');
    expect(value('account')).toBe('ben');
    expect(value('api')).toBe('2 keys');
    expect(value('people')).toBe('3 users');
    expect(value('database')).toBe('PostgreSQL');
  });

  /// A value that has not loaded yet must render as nothing at all. A placeholder or a zero
  /// would be a claim about the instance -- "no tokens", "SQLite" -- that nothing has checked.
  it('shows no value where the answer is not known yet, rather than guessing one', () => {
    const rows = settingsRows(input({ isAdmin: true, tokenCount: null, userCount: null, backendLabel: null }));
    const value = (id: string) => rows.find((r) => r.id === id)?.value;
    expect(value('api')).toBeNull();
    expect(value('people')).toBeNull();
    expect(value('database')).toBeNull();
  });

  it('says nothing rather than "0 keys" when there are none', () => {
    expect(settingsRows(input({ tokenCount: 0 })).find((r) => r.id === 'api')?.value).toBeNull();
  });

  it('never gives the data row a value, because it is a pair of actions and not a state', () => {
    expect(settingsRows(input()).find((r) => r.id === 'data')?.value).toBeNull();
  });

  it('routes every row under /settings', () => {
    for (const row of settingsRows(input({ isAdmin: true }))) {
      expect(row.path.startsWith('/settings/')).toBe(true);
    }
  });

  it('survives a session with no username, which is the state before /auth/me answers', () => {
    expect(settingsRows(input({ username: null })).find((r) => r.id === 'account')?.value).toBeNull();
  });
});
```

- [ ] **Step 2: Run and watch it fail**

Run: `cd frontend && npx vitest run tests/settings-rows.test.ts`

Expected: FAIL — `Failed to resolve import "../src/lib/settings-rows"`.

- [ ] **Step 3: Write the module**

Create `frontend/src/lib/settings-rows.ts`:

```ts
import type { IconName } from './Icon.svelte';

export type SettingsGroup = 'you' | 'instance';

/** One row of the settings hub. `label` is an i18n key; `value` is text already rendered for
 *  display, or `null` to show nothing at all. */
export type SettingsRowModel = {
  id: string;
  path: string;
  icon: IconName;
  label: string;
  value: string | null;
  group: SettingsGroup;
};

export type SettingsRowsInput = {
  isAdmin: boolean;
  username: string | null;
  /** Already-translated, e.g. "Dark" -- this module does no lookups. */
  themeLabel: string;
  /** An uppercase language code, e.g. "EN". */
  localeLabel: string;
  /** `null` until `/auth/tokens` answers. */
  tokenCount: number | null;
  /** `null` until `/users` answers, and for a non-admin who never asks. */
  userCount: number | null;
  /** `null` until `/database` answers, e.g. "PostgreSQL". */
  backendLabel: string | null;
};

/** What the hub shows, and what each row says about itself.
 *
 *  Every value here is either a fact the caller already has or `null`. A row whose answer has
 *  not arrived shows nothing rather than a placeholder: "SQLite" or "0 keys" on a screen is a
 *  claim about the instance, and a hub that guesses is worse than one that waits. */
export function settingsRows(input: SettingsRowsInput): SettingsRowModel[] {
  const rows: SettingsRowModel[] = [
    {
      id: 'appearance', path: '/settings/appearance', icon: 'palette', label: 'settings.appearance',
      value: `${input.themeLabel} · ${input.localeLabel}`, group: 'you',
    },
    {
      id: 'account', path: '/settings/account', icon: 'person', label: 'settings.account',
      value: input.username, group: 'you',
    },
    {
      id: 'api', path: '/settings/api', icon: 'key', label: 'tokens.title',
      // Zero is not a number worth printing here: "no keys" is the default state of every
      // account, and a row that says so is noise on a screen meant to be scanned.
      value: input.tokenCount ? `${input.tokenCount} keys` : null, group: 'you',
    },
    {
      id: 'data', path: '/settings/data', icon: 'box', label: 'settings.data',
      value: null, group: 'you',
    },
  ];
  if (!input.isAdmin) return rows;
  rows.push(
    {
      id: 'people', path: '/settings/people', icon: 'people', label: 'settings.users',
      value: input.userCount ? `${input.userCount} users` : null, group: 'instance',
    },
    {
      id: 'database', path: '/settings/database', icon: 'database', label: 'db.title',
      value: input.backendLabel, group: 'instance',
    },
  );
  return rows;
}
```

- [ ] **Step 4: Run the tests**

Run: `cd frontend && npx vitest run tests/settings-rows.test.ts`

Expected: PASS, 8 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/settings-rows.ts frontend/tests/settings-rows.test.ts
git commit -m "feat: decide what the settings hub shows, and what each row says"
```

---

### Task 3: Appearance and Account

**Files:**
- Create: `frontend/src/routes/settings/Appearance.svelte`, `frontend/src/routes/settings/Account.svelte`
- Modify: `frontend/src/App.svelte`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`

**Interfaces:**
- Produces: routes `/settings/appearance` and `/settings/account`.

Both pages lift their content **verbatim** out of `frontend/src/routes/Settings.svelte`. Open that file and move, do not retype — a transcription that "tidies" a string or drops a hint is a behaviour change smuggled into a reorganisation. The old sections stay in `Settings.svelte` for now; Task 6 removes them when the hub replaces that screen.

- [ ] **Step 1: Add the two i18n keys, both languages**

`frontend/src/i18n/en.ts`, beside the existing `settings.*` keys:

```ts
  'settings.appearance': 'Appearance',
  'settings.you': 'You',
  'settings.instance': 'This instance',
```

`frontend/src/i18n/de.ts`, same position:

```ts
  'settings.appearance': 'Darstellung',
  'settings.you': 'Du',
  'settings.instance': 'Diese Instanz',
```

(`settings.you` and `settings.instance` are the hub's group headings. They are added here so Task 6 does not have to touch two locale files for two strings.)

- [ ] **Step 2: Write `Appearance.svelte`**

Create `frontend/src/routes/settings/Appearance.svelte`. It carries the **Language** and **Theme** sections, and — for an administrator only — the **Currency** section, all exactly as they appear in `Settings.svelte` today.

From `Settings.svelte`'s `<script>` it needs: the `settings` store import, the `t`/`locale` i18n imports, `LANG_NAMES`/`SUPPORTED` from `../../i18n/detect`, `currency` and `user` from `../../stores/session`, `api` from `../../lib/api`, the `currencyText`/`message`/`error` state, the `isAdmin` derivation, `saveCurrency()`, and the `onMount` line that seeds `currencyText = $currency`.

Note the import paths gain one `../` — the file sits a directory deeper than `Settings.svelte` does.

Its markup is:

```svelte
<main>
  <TopBar title={$t('settings.appearance')} backTo="/settings" />
  {#if error}<p class="error">{error}</p>{/if}
  {#if message}<p class="muted">{message}</p>{/if}
  <!-- the Language section, verbatim from Settings.svelte -->
  <!-- the Theme section, verbatim from Settings.svelte -->
  {#if isAdmin}
    <!-- the Currency section, verbatim from Settings.svelte, plus the hint below -->
    <p class="hint">{$t('settings.currency-everyone')}</p>
  {/if}
</main>
```

Add the new hint key to both locales — `en.ts`: `'settings.currency-everyone': 'The currency applies to everyone on this instance.'`; `de.ts`: `'settings.currency-everyone': 'Die Währung gilt für alle auf dieser Instanz.'` Currency is an instance-wide setting sitting on a page that otherwise holds personal preferences, and without that sentence an administrator would reasonably read it as their own.

- [ ] **Step 3: Write `Account.svelte`**

Create `frontend/src/routes/settings/Account.svelte` carrying the **Account** section verbatim: the username line, the change-password field and button, the sign-out button, the sign-out-everywhere button, and the `settings.logout-all-hint` paragraph beneath them.

From `Settings.svelte`'s `<script>` it needs: `api`, `t`, `user`/`logout`/`logoutEverywhere` from the session store, the `ownPass`/`message`/`error` state, `changeOwnPassword()`, and `signOutEverywhere()` — including its `confirm()`, which must not be dropped.

Its `TopBar` is `<TopBar title={$t('settings.account')} backTo="/settings" />`.

- [ ] **Step 4: Register both routes**

In `frontend/src/App.svelte`, import the two components beside the existing route imports:

```ts
  import SettingsAppearance from './routes/settings/Appearance.svelte';
  import SettingsAccount from './routes/settings/Account.svelte';
```

and add two entries to the `routes` table, immediately after the existing `['/settings', Settings],` line:

```ts
    ['/settings/appearance', SettingsAppearance],
    ['/settings/account', SettingsAccount],
```

The route matcher splits on `/` and requires equal segment counts, so `/settings/appearance` cannot collide with `/settings`.

- [ ] **Step 5: Verify both pages load and work**

Run: `cd frontend && npm run check && npm run build`

Expected: 0 errors, 0 warnings; build clean.

Then check by hand with Playwright (a throwaway script or temporary spec you delete afterwards — do not leave one in `tests-e2e/`): sign in, visit `/settings/appearance`, change the theme, confirm the page visibly changes and the setting sticks across a reload; visit `/settings/account` and confirm the username shows. Report what you did and what you saw.

- [ ] **Step 6: Run the suites**

Run: `cd frontend && npx vitest run && npx playwright test`

Expected: vitest unchanged (186 + Task 2's 8 = 194); Playwright 78 unchanged — the old `Settings.svelte` is untouched, so every existing spec still drives it exactly as before.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/routes/settings/Appearance.svelte frontend/src/routes/settings/Account.svelte frontend/src/App.svelte frontend/src/i18n
git commit -m "feat: appearance and account get their own pages"
```

---

### Task 4: API access and Your data

**Files:**
- Create: `frontend/src/routes/settings/ApiAccess.svelte`, `frontend/src/routes/settings/Data.svelte`
- Modify: `frontend/src/App.svelte`

**Interfaces:**
- Produces: routes `/settings/api` and `/settings/data`.

Same rule as Task 3: lift the content verbatim out of `frontend/src/routes/Settings.svelte`, leaving the originals in place for Task 6 to remove.

- [ ] **Step 1: Write `ApiAccess.svelte`**

Create `frontend/src/routes/settings/ApiAccess.svelte` carrying the whole **API tokens** section verbatim: the intro paragraph, the fresh-token card, the token list with its revoke buttons and last-used lines, the name field, the create button, and the `tokens.password-note` line.

From `Settings.svelte`'s `<script>`: `api`, `t`, `locale`, `fmtDate`, the `ApiToken` type, the `tokens`/`tokenName`/`freshToken`/`copied`/`error` state **including the doc comment above `freshToken`** (it explains that the server returns the plaintext once and stores only a hash — that comment is the reason the card exists and must travel with it), `loadTokens()`, `createToken()`, `copyToken()` **with its comment about clipboard permission**, `revokeToken()` **with its `confirm()`**, and an `onMount` that calls `loadTokens()`.

Also move the `.fresh-token` CSS rules from `Settings.svelte`'s `<style>` block, including the comment explaining why the token is allowed to wrap (it can never be shown again).

`TopBar`: `<TopBar title={$t('tokens.title')} backTo="/settings" />`.

- [ ] **Step 2: Write `Data.svelte`**

Create `frontend/src/routes/settings/Data.svelte` carrying the **Data** section verbatim: the export link, the import button and its hidden file input, and the `settings.export-not-backup` paragraph **with its comment** explaining why that sentence sits beside the button rather than in the backup section.

From `Settings.svelte`'s `<script>`: `uploadRaw`, `t`, the `ImportCounts` type, the `fileEl` binding, the `message`/`error` state, and `doImport()`.

`TopBar`: `<TopBar title={$t('settings.data')} backTo="/settings" />`.

- [ ] **Step 3: Register both routes**

In `frontend/src/App.svelte`, beside the imports added by the previous task:

```ts
  import SettingsApiAccess from './routes/settings/ApiAccess.svelte';
  import SettingsData from './routes/settings/Data.svelte';
```

and in the `routes` table, after the two entries the previous task added:

```ts
    ['/settings/api', SettingsApiAccess],
    ['/settings/data', SettingsData],
```

- [ ] **Step 4: Verify**

Run: `cd frontend && npm run check && npm run build && npx vitest run && npx playwright test`

Expected: check 0/0, build clean, vitest 194, Playwright 78.

Then exercise both pages by hand with a throwaway Playwright script you delete afterwards: create a token at `/settings/api` and confirm the plaintext appears once and the list grows; revoke it and confirm the confirmation appears and the row goes. Visit `/settings/data` and confirm the export link points at `/api/export`. Report what you saw.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/routes/settings/ApiAccess.svelte frontend/src/routes/settings/Data.svelte frontend/src/App.svelte
git commit -m "feat: API access and your data get their own pages"
```

---

### Task 5: People and Database, and the admin redirect

**Files:**
- Create: `frontend/src/routes/settings/People.svelte`, `frontend/src/routes/settings/Database.svelte`
- Modify: `frontend/src/App.svelte`

**Interfaces:**
- Produces: routes `/settings/people` and `/settings/database`, both admin-only.

- [ ] **Step 1: Write `People.svelte`**

Create `frontend/src/routes/settings/People.svelte` carrying the **Users** and **New user** sections verbatim: the user list with its admin marker and delete buttons, and the new-user name field, password field, admin checkbox and create button with their existing `disabled` conditions (`newName.length < 3 || newPass.length < 8`).

From `Settings.svelte`'s `<script>`: `api`, `t`, the `User` type, `user` from the session store, the `users`/`newName`/`newPass`/`newAdmin`/`error` state, `loadUsers()`, `addUser()`, `removeUser()` **with its `confirm()`**, and an `onMount` calling `loadUsers()`. Also move the `.toggle input` CSS rule that sizes the admin checkbox.

`TopBar`: `<TopBar title={$t('settings.users')} backTo="/settings" />`.

- [ ] **Step 2: Write `Database.svelte`**

Create `frontend/src/routes/settings/Database.svelte` carrying the **Database** section *and* the **Backup** section verbatim — both, on one page. The spec is explicit about why: the backup section exists to say that on PostgreSQL LogB backs nothing up, and an instance migrated by the controls immediately above it looks in every other respect exactly as healthy as before. Splitting them is how somebody migrates and never reads the consequence.

From `Settings.svelte`'s `<script>`: `api`, `ApiError`, `t`, `locale`, `fmtDate`, the `BackupStatus`/`DbDescription`/`DbLocation`/`DbProbe`/`DbSwitched` types, the `db`/`backup`/`dbUrl`/`probe`/`probing`/`switching`/`switched`/`dbError`/`restarting`/`restartNote` state **with every one of their doc comments**, the `SWITCH_TIMEOUT_MS` constant **with its comment**, the `canChooseDb` derivation **with its comment**, `loadDatabase()`, `loadBackup()`, `hourText()`, `backendName()`, `place()`, `testDatabase()`, `switchDatabase()` **with its comment about why an interrupted switch is not reported as a failure**, `restartNow()` **with its `confirm()`**, and an `onMount` calling `loadDatabase()` and `loadBackup()`.

Every comment in this section was written against a specific way an operator can lose data. Move all of them, including the ones in the markup: the blob-warning comment, the "above the field, not below it" comment, the restart-button comment about not promising a supervisor, and the three-state comment above the backup card.

Move these CSS rules too, with their comments: `.stack`, `.break`, `.blobs`, `.blobs b`, `.restart`, `.elsewhere`, `.elsewhere b`, `.tables`, `.tables li`.

`TopBar`: `<TopBar title={$t('db.title')} backTo="/settings" />`.

- [ ] **Step 3: Register the routes and guard them**

In `frontend/src/App.svelte`, add the imports and the two route entries as in the previous tasks:

```ts
  import SettingsPeople from './routes/settings/People.svelte';
  import SettingsDatabase from './routes/settings/Database.svelte';
```

```ts
    ['/settings/people', SettingsPeople],
    ['/settings/database', SettingsDatabase],
```

Then add the redirect. `App.svelte` already has a route-guard `$effect` handling setup and login; add a second one beneath it:

```svelte
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
```

- [ ] **Step 4: Verify, including the redirect**

Run: `cd frontend && npm run check && npm run build && npx vitest run && npx playwright test`

Expected: check 0/0, build clean, vitest 194, Playwright 78.

Then prove the redirect with a throwaway Playwright script you delete afterwards: sign in as the admin, create a second non-admin user at `/settings/people`, sign out, sign in as that user, navigate to `/settings/database`, and confirm you land on `/settings`. Report the URL you observed. A redirect that silently did not happen would leave a non-admin staring at a database migration form, so do not skip this.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/routes/settings/People.svelte frontend/src/routes/settings/Database.svelte frontend/src/App.svelte
git commit -m "feat: people and database get their own pages, for admins only"
```

---

### Task 6: The hub

**Files:**
- Create: `frontend/src/lib/SettingsRow.svelte`, `frontend/tests-e2e/14-settings.spec.ts`
- Modify: `frontend/src/routes/Settings.svelte` (rewritten), `frontend/src/app.css`

**Interfaces:**
- Consumes: `settingsRows`, `SettingsRowModel`, `SettingsRowsInput` from Task 2; the six routes from Tasks 3–5; the seven icons from Task 1.

- [ ] **Step 1: Write `SettingsRow.svelte`**

Create `frontend/src/lib/SettingsRow.svelte`:

```svelte
<script lang="ts">
  import Icon from './Icon.svelte';
  import { go } from './router';
  import { t } from '../i18n';
  import type { SettingsRowModel } from './settings-rows';

  let { row }: { row: SettingsRowModel } = $props();
</script>

<button class="settings-row" onclick={() => go(row.path)}>
  <span class="icon"><Icon name={row.icon} /></span>
  <span class="label">{$t(row.label)}</span>
  <!-- A row with no value renders no element at all, rather than an empty one: an empty span
       still takes its grid column and leaves the chevron sitting away from the edge, which
       reads as a value that failed to load rather than one that was never there. -->
  {#if row.value}<span class="value muted">{row.value}</span>{/if}
  <span class="chev muted"><Icon name="chevron" size={18} /></span>
</button>

<style>
  .settings-row {
    display: grid; grid-template-columns: auto 1fr auto auto;
    align-items: center; gap: var(--space-3);
    width: 100%; text-align: left; background: var(--surface);
    border: 1px solid var(--border); border-radius: var(--radius-md);
    padding: var(--space-3); min-height: var(--control);
  }
  .icon { display: flex; color: var(--muted); }
  .value { font-size: var(--text-sm); }
  .chev { display: flex; }
</style>
```

- [ ] **Step 2: Rewrite `Settings.svelte` as the hub**

Replace the whole of `frontend/src/routes/Settings.svelte`. Everything it used to hold now lives on the six pages from Tasks 3–5; what stays is the hub and the failed-sync banner.

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import SettingsRow from '../lib/SettingsRow.svelte';
  import { api, deadOps, discardDeadOp, retryDead } from '../lib/api';
  import { t } from '../i18n';
  import { settings } from '../stores/settings';
  import { user } from '../stores/session';
  import { settingsRows } from '../lib/settings-rows';
  import type { ApiToken, DbDescription, User } from '../lib/types';
  import type { QueuedOp } from '../lib/outbox';

  let dead = $state<QueuedOp[]>([]);
  let tokenCount = $state<number | null>(null);
  let userCount = $state<number | null>(null);
  let backendLabel = $state<string | null>(null);
  let error = $state('');

  const isAdmin = $derived($user?.is_admin === true);

  /** The hub's rows, values and all. Everything here is a fact already in hand -- a count that
   *  has not arrived yet stays `null`, and its row simply shows nothing. */
  const rows = $derived(settingsRows({
    isAdmin,
    username: $user?.username ?? null,
    themeLabel: $t(`settings.theme-${$settings.theme}`),
    localeLabel: ($settings.locale === 'auto' ? navigator.language : $settings.locale).slice(0, 2).toUpperCase(),
    tokenCount, userCount, backendLabel,
  }));

  // Each of these fills in one row's value. They fail quietly: a hub whose Database row says
  // nothing is honest, while a hub that shows an error banner because a count did not load
  // makes a working instance look broken.
  onMount(async () => {
    dead = await deadOps();
    try { tokenCount = (await api<ApiToken[]>('GET', '/auth/tokens')).length; } catch { /* row shows nothing */ }
    if (!isAdmin) return;
    try { userCount = (await api<User[]>('GET', '/users')).length; } catch { /* row shows nothing */ }
    try {
      const db = await api<DbDescription>('GET', '/database');
      backendLabel = $t(db.backend === 'postgres' ? 'db.backend-postgres' : 'db.backend-sqlite');
    } catch { /* row shows nothing */ }
  });

  async function retryOutbox() {
    await retryDead();
    dead = await deadOps();
  }

  /** A permanently-rejected op (e.g. an upload naming an activity that will never exist) can
   *  never succeed no matter how many times "Try again" is pressed -- this is the only way to
   *  make it leave IndexedDB (and, for an upload, release its file bytes) short of the user
   *  clearing site data entirely. */
  async function discardOp(id: string) {
    if (!confirm($t('nav.confirm-delete'))) return;
    await discardDeadOp(id);
    dead = await deadOps();
  }
</script>

<main>
  <TopBar title={$t('settings.title')} backTo="/" />
  {#if error}<p class="error">{error}</p>{/if}

  <!-- Not a row in a group: a failed write is an alert, it is the only time-sensitive thing on
       this screen, and it is absent entirely when the queue is clean. It expands here rather
       than behind a route of its own -- a URL for a screen that is almost always empty would be
       a seventh destination that exists to be blank. -->
  {#if dead.length > 0}
    <div class="banner">
      <b>{$t('outbox.failed')}</b>
    </div>
    <div class="list">
      {#each dead as op (op.id)}
        <div class="card row">
          <span>
            <b>{String(op.body.title ?? op.kind)}</b>
            <span class="muted">{op.path}</span>
          </span>
          <button class="ghost danger-text" onclick={() => discardOp(op.id)}>{$t('outbox.discard')}</button>
        </div>
      {/each}
    </div>
    <button onclick={retryOutbox}>{$t('outbox.retry')}</button>
  {/if}

  <h2>{$t('settings.you')}</h2>
  <div class="settings-grid">
    {#each rows.filter((r) => r.group === 'you') as row (row.id)}<SettingsRow {row} />{/each}
  </div>

  {#if isAdmin}
    <h2>{$t('settings.instance')}</h2>
    <div class="settings-grid">
      {#each rows.filter((r) => r.group === 'instance') as row (row.id)}<SettingsRow {row} />{/each}
    </div>
  {/if}
</main>

<style>
  .danger-text { color: var(--danger); }
</style>
```

Note `settings.theme-auto`/`-light`/`-dark` already exist as keys (the theme picker uses them), so `settings.theme-${$settings.theme}` resolves for all three values.

- [ ] **Step 3: Add the grid**

Append to `frontend/src/app.css`, after the shell block:

```css
/* The hub's rows: a single column on a phone, two where there is room. Six rows in one column
   on a wide screen would leave most of the pane empty and put the last row a long way from the
   first. */
.settings-grid { display: grid; gap: var(--space-2); }
@media (width >= 900px) {
  .settings-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
}
```

- [ ] **Step 4: Write the end-to-end spec**

Create `frontend/tests-e2e/14-settings.spec.ts`:

```ts
import { test, expect } from '@playwright/test';
import { signIn } from './helpers';

test('the hub lists what there is, and each row opens its own page', async ({ page }) => {
  await signIn(page);
  await page.getByRole('navigation', { name: /Main|Hauptnavigation/ }).getByRole('button', { name: 'Settings' }).click();
  await expect(page).toHaveURL(/\/settings$/);

  for (const [name, url] of [
    ['Appearance', /\/settings\/appearance$/],
    ['Account', /\/settings\/account$/],
    ['API tokens', /\/settings\/api$/],
  ] as const) {
    await page.getByRole('button', { name: new RegExp(name) }).click();
    await expect(page).toHaveURL(url);
    await page.goBack();
    await expect(page).toHaveURL(/\/settings$/);
  }
});

/// The value on a row is the reason the hub is a hub rather than a menu: it answers the
/// question without being opened.
test('a row carries its current value', async ({ page }) => {
  await signIn(page);
  await page.goto('/settings');
  // `signIn` uses the admin account, so the account row shows its username.
  await expect(page.getByRole('button', { name: /Account/ })).toContainText('ben');
});

test('an administrator sees the instance group; the rows are real links', async ({ page }) => {
  await signIn(page);
  await page.goto('/settings');
  await expect(page.getByRole('button', { name: /Database/ })).toBeVisible();
  await page.getByRole('button', { name: /Database/ }).click();
  await expect(page).toHaveURL(/\/settings\/database$/);
  // The database page carries the backup section too -- they are one page deliberately.
  await expect(page.getByText(/Backup|Sicherung/).first()).toBeVisible();
});
```

Check the accessible names against the real rendered rows before finalising — a row's accessible name is its icon (none, `aria-hidden`), its label, its value and the chevron concatenated, so `{ name: /Account/ }` matches by substring while an exact string would not. If a name does not match, fix the selector rather than the component, unless the component is genuinely producing a bad name.

- [ ] **Step 5: Run everything**

Run: `cd frontend && npm run check && npm run build && npx vitest run && npx playwright test`

Expected: check 0/0; build clean; vitest 194; Playwright 78 + 3 new × 2 projects = 84.

**Existing specs that drove the old single-page Settings will fail here** — `01-smoke`, `07-api-tokens`, `10-database` and `04-offline` all click "Settings" and then expect a control that now lives one page deeper. Fix each by navigating through the hub to the right page. **Move the assertions; do not delete them.** A behaviour that loses its test during this reorganisation is a behaviour this reorganisation silently dropped. Report every spec you touched and what you changed.

- [ ] **Step 6: Look at it**

Capture the hub with Playwright at both viewports, in both themes: `/tmp/hub-mobile-light.png`, `/tmp/hub-mobile-dark.png`, `/tmp/hub-desktop-light.png`, `/tmp/hub-desktop-dark.png`, plus one sub-page at desktop width (`/tmp/hub-desktop-database.png`).

Report what you actually see. Specifically: are the values legible and clearly secondary to the labels; does the two-column grid look deliberate at 1280px or merely wide; is the chevron aligned consistently across rows of differing value lengths; and does a row with no value look intentional rather than broken. Fix what is wrong, using existing tokens only.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/lib/SettingsRow.svelte frontend/src/routes/Settings.svelte frontend/src/app.css frontend/tests-e2e frontend/src/i18n
git commit -m "feat: settings becomes a hub of six pages"
```

---

## Self-Review

**Spec coverage.** Walking the spec: *the hub and its six pages* — Tasks 3–6. *Rows carry their current value, `null` when unknown* — Task 2's model, Task 6's wiring, proven by both the unit tests and an e2e assertion. *Two groups, instance only for admins* — Task 2, Task 6. *The failed-sync banner expands in place with no route* — Task 6 Step 2. *Appearance holds currency with a hint that it is instance-wide* — Task 3 Step 2. *Account, API access, Your data* — Tasks 3 and 4. *People* — Task 5. *Database and backup on one page* — Task 5 Step 2, with the reasoning restated so the implementer does not "improve" it into two. *Non-admin redirect* — Task 5 Step 3, proved in Step 4. *Seven icons, no emoji stand-ins* — Task 1. *Desktop two-column grid, no third nav column* — Task 6 Step 3. *Destructive actions keep confirmations* — named explicitly in Tasks 3, 4 and 5 for each `confirm()` that moves. *Tests: unit row model, e2e on both projects, moved not deleted* — Tasks 2 and 6.

One spec item deserved a task and now has one: "every existing behaviour moved keeps its current tests… a behaviour that loses its test during this move is a behaviour this redesign silently dropped." That is Task 6 Step 5, stated as a requirement with a report obligation rather than left as an aspiration.

**Placeholder scan.** No TBD, no "handle errors appropriately". The extraction steps say "move verbatim from `Settings.svelte`" and name every function, state variable, comment and CSS rule that must travel — for a move, naming the parts is complete and retyping 450 existing lines into the plan would invite transcription drift from the real source. Steps that ask the implementer to check something and report (Task 3 Step 5, Task 5 Step 4, Task 6 Step 6) do so because the answer depends on runtime behaviour the plan cannot know, and each says exactly what to check.

**Type consistency.** `SettingsRowModel`, `SettingsRowsInput`, `SettingsGroup`, `settingsRows` are used identically in Tasks 2 and 6. The seven icon names in Task 1's union are exactly the ones Task 2's model emits (`palette`, `person`, `key`, `box`, `people`, `database`) plus `chevron`, which only `SettingsRow.svelte` uses. Route paths in Task 2's model (`/settings/appearance`, `/settings/account`, `/settings/api`, `/settings/data`, `/settings/people`, `/settings/database`) match the route-table entries registered in Tasks 3, 4 and 5 exactly, and match `ADMIN_ONLY` in Task 5. `db.backend-postgres`/`db.backend-sqlite` are existing keys, used by Task 6 the same way `Database.svelte`'s `backendName()` uses them.
