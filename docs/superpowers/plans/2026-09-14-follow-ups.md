# Follow-ups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix a flaky spec, stop the object page rewriting its URL, keep the Statistics year in the URL, offer include-contents for archived-only children, and replace five copies of one CSS rule with one.

**Architecture:** Frontend-only changes: one test fix, one global CSS rule, two URL-sync effects, one extra children request.

**Tech Stack:** Svelte 5 runes, TypeScript, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-14-follow-ups-design.md`

## Global Constraints

- Work on branch `build-follow-ups` (already created from `main`); never commit to `main`. Before every commit run `git branch --show-current` and commit only if it prints `build-follow-ups` — another session shares this repository through a separate worktree.
- Commands run in the foreground; never end a turn with a background command pending. Playwright commands take minutes (they build the frontend and the Rust binary): timeouts up to 600000 ms. If memory is short, add `--workers=1`.
- Never weaken an assertion; re-run a failing pre-existing spec alone once and report both runs.
- Comments explain *why*.
- Commits end with exactly:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DNUZLftSTtND7ABvN6eoGv
  ```

---

### Task 1: Flaky settings spec and one checkbox rule

**Files:**
- Modify: `frontend/tests-e2e/14-settings.spec.ts`, `frontend/src/app.css`, `frontend/src/routes/ObjectForm.svelte`, `frontend/src/routes/ReminderForm.svelte`, `frontend/src/routes/settings/People.svelte`, `frontend/src/routes/Stats.svelte`, `frontend/src/lib/Insights.svelte`

- [ ] **Step 1: Wait for the user list**

In `frontend/tests-e2e/14-settings.spec.ts`, test `a row carries its current value`, directly after `await page.goto('/settings/people');` add:

```ts
  // The list loads after the page renders. Counting Remove buttons before it arrives finds none,
  // skips the cleanup, and the assertion below then reads every user earlier specs left behind.
  await expect(page.locator('.card.row', { hasText: 'ben' })).toBeVisible();
```

- [ ] **Step 2: Move the checkbox rule into app.css**

In `frontend/src/app.css`, directly after the line `.row > * { flex: 1; }` add:

```css
/* A checkbox or radio in a toggle row keeps its own size: the rule above would otherwise stretch
   it across half the row. */
.row.toggle input { flex: none; width: 20px; height: 20px; }
```

Delete the line `.toggle input { flex: none; width: 20px; height: 20px; }` from the `<style>` of each of: `ObjectForm.svelte`, `ReminderForm.svelte`, `settings/People.svelte`, `Stats.svelte`, `lib/Insights.svelte`. Also delete any comment line directly above it that explains only that rule (Stats.svelte and Insights.svelte have one). If a `<style>` block becomes empty, delete the block. In Stats.svelte, if the remaining style comment lists which classes are global, keep it accurate.

Check: `grep -rn "toggle input" frontend/src` prints only the app.css line.

- [ ] **Step 3: Verify**

1. `cd frontend && npm run check && npx vitest run` — 0 errors, all pass.
2. `cd frontend && npm run e2e -- 11-controls 14-settings 20-statistics 21-object-cost-depth` — all pass.
3. `cd frontend && npx playwright test 14-settings --repeat-each=3` — all pass (flake fixed).
4. A 390px screenshot of `/settings/people` (admin signed in) showing the "Administrator" checkbox beside its label, saved to `/home/ben/repo/logb/.superpowers/sdd/fu-people-390.png`; look at it.

- [ ] **Step 4: Commit**

```bash
git add frontend/tests-e2e/14-settings.spec.ts frontend/src/app.css frontend/src/routes/ObjectForm.svelte frontend/src/routes/ReminderForm.svelte frontend/src/routes/settings/People.svelte frontend/src/routes/Stats.svelte frontend/src/lib/Insights.svelte
git commit -m "fix: settings spec waits for the user list; toggle rows share one checkbox rule"
```

---

### Task 2: Addresses — object tab and statistics year

**Files:**
- Modify: `frontend/src/routes/ObjectDetail.svelte`, `frontend/src/routes/Stats.svelte`, `frontend/tests-e2e/20-statistics.spec.ts`

- [ ] **Step 1: Failing e2e for the year**

In `frontend/tests-e2e/20-statistics.spec.ts`, in the first test, directly after the existing lines that select `2026` and assert the total contains `750.00` (and before the twelve-month bar count), add:

```ts
  // The year is kept in the address, so a reload -- or coming back from an object -- keeps it.
  await expect(page).toHaveURL(/[?&]year=2026(&|$)/);
  await page.reload();
  await expect(page.getByLabel('Year')).toHaveValue('2026');
  await expect(page.getByTestId('stats-total')).not.toContainText('1,750.00');
  await expect(page.getByTestId('stats-total')).toContainText('750.00');
```

If `total` is a locator variable declared earlier, the existing later assertions keep working after the reload (locators re-resolve).

Run: `cd frontend && npm run e2e -- 20-statistics`
Expected: FAIL at `toHaveURL`.

- [ ] **Step 2: Year in the URL**

In `frontend/src/routes/Stats.svelte`, replace `let year = $state<string | null>(null);` with:

```ts
  /** Four digits from `?year=`, or all years. Anything else in the address is ignored rather than
   *  sent to the API to be refused. */
  const yearFromUrl = new URLSearchParams(location.search).get('year');
  let year = $state<string | null>(yearFromUrl && /^\d{4}$/.test(yearFromUrl) ? yearFromUrl : null);

  // The year stays in the address, as Search keeps its query: a reload, a shared link or the back
  // button from an object returns to the same year. Replaced only when it changes, so opening
  // `/stats` does not rewrite its own address.
  $effect(() => {
    const url = new URL(location.href);
    if (year) url.searchParams.set('year', year); else url.searchParams.delete('year');
    const next = url.pathname + url.search;
    if (next !== location.pathname + location.search) history.replaceState(null, '', next);
  });
```

- [ ] **Step 3: Default tab out of the URL**

In `frontend/src/routes/ObjectDetail.svelte`, replace:

```ts
  $effect(() => {
    const url = new URL(location.href);
    url.searchParams.set('tab', tab);
    history.replaceState(null, '', url.pathname + url.search);
  });
```

with:

```ts
  // The default tab is left out of the address, and the address is replaced only when it changes:
  // opening `/objects/5` must not turn into `/objects/5?tab=timeline` a moment later, which
  // breaks a back-button history entry's match and any test waiting for the plain URL.
  $effect(() => {
    const url = new URL(location.href);
    if (tab === 'timeline') url.searchParams.delete('tab'); else url.searchParams.set('tab', tab);
    const next = url.pathname + url.search;
    if (next !== location.pathname + location.search) history.replaceState(null, '', next);
  });
```

- [ ] **Step 4: Drop the spec workaround**

In `frontend/tests-e2e/20-statistics.spec.ts`, the final step wraps the click on the "Stats Car" button and `page.waitForURL(`**/objects/${car}`)` in `Promise.all` with a comment about the URL being rewritten. Replace that block with the click followed by `await page.waitForURL(`**/objects/${car}`);`, and delete the comment explaining the old race.

- [ ] **Step 5: Verify**

1. `cd frontend && npm run check && npx vitest run`
2. `cd frontend && npm run e2e -- 02-lifecycle 12-object-hierarchy 20-statistics 21-object-cost-depth` — all pass.
3. `grep -rn "tab=timeline" frontend/src frontend/tests-e2e` — no hits.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/routes/ObjectDetail.svelte frontend/src/routes/Stats.svelte frontend/tests-e2e/20-statistics.spec.ts
git commit -m "fix: object page keeps its plain address; statistics year lives in the URL"
```

---

### Task 3: Include-contents switch for archived-only children

**Files:**
- Modify: `frontend/src/routes/ObjectDetail.svelte`, `frontend/tests-e2e/21-object-cost-depth.spec.ts`, `docs/superpowers/specs/2026-09-14-object-cost-depth-design.md`, `docs/superpowers/specs/2026-09-14-follow-ups-design.md`

- [ ] **Step 1: Failing e2e**

Append to `frontend/tests-e2e/21-object-cost-depth.spec.ts` (the file already defines `object`, `entry`, `openInfo`):

```ts
test('a house whose only child is archived still offers to include it', async ({ page }) => {
  await signInFresh(page, '21-cost-archived');
  const house = await object(page, { name: 'Quiet House', type: 'home' });
  const shed = await object(page, { name: 'Old Shed', type: 'other', parent_id: house });
  await entry(page, house, { date: '2025-03-10', category: 'repair', cost_cents: 50_000 });
  await entry(page, shed, { date: '2025-04-10', category: 'repair', cost_cents: 10_000 });
  // PATCH replaces the object, so the whole body is sent with archived set.
  const res = await page.request.patch(`/api/objects/${shed}`, {
    data: { name: 'Old Shed', type: 'other', description: '', parent_id: house, archived: true },
  });
  expect(res.ok()).toBe(true);

  await openInfo(page, house);
  const ownership = page.getByTestId('insights-ownership');
  await expect(ownership).toContainText('500.00');
  const toggle = page.getByLabel('Include contents');
  await expect(toggle).toBeVisible();
  await toggle.check();
  await expect(ownership).toContainText('600.00');
});
```

(The switch state is remembered per device; a fresh browser context per test starts it off.)

Run: `cd frontend && npm run e2e -- 21-object-cost-depth`
Expected: the new test FAILS at `toBeVisible`.

- [ ] **Step 2: Fetch archived children**

In `frontend/src/routes/ObjectDetail.svelte`, next to `let children = $state<MemObject[]>([]);` add:

```ts
  /** Archived children are not listed under Contents, but their costs still count with "Include
   *  contents", so having any is enough to offer the switch. */
  let archivedChildCount = $state(0);
```

In `loadChildren`, after the existing request for active children, add:

```ts
    try { archivedChildCount = (await api<MemObject[]>('GET', `/objects?parent_id=${oid}&archived=true`)).length; }
    catch { archivedChildCount = 0; }
```

(keep whatever error handling the existing active-children request has; this second request failing only hides the switch.)

Change the Insights usage to:

```svelte
      <Insights objectId={oid} unit={object.counter_unit} hasContents={children.length > 0 || archivedChildCount > 0} />
```

- [ ] **Step 3: Specs**

In `docs/superpowers/specs/2026-09-14-object-cost-depth-design.md` block 1, reword the switch visibility to: shown when the object has at least one non-deleted child, archived or not (archived children are not listed under Contents but still count). Remove the sentence saying a house whose only children are archived shows no switch.

In `docs/superpowers/specs/2026-09-14-follow-ups-design.md`, set the status line to `Status: implemented.`

- [ ] **Step 4: Verify**

1. `cd frontend && npm run check && npx vitest run`
2. `cd frontend && npm run e2e -- 21-object-cost-depth 12-object-hierarchy` — all pass (21 now 6 = 3 tests × 2 projects).
3. Full suite: `cd frontend && npm run e2e -- --workers=2` — all pass; re-run any failing spec alone once and report both runs.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/routes/ObjectDetail.svelte frontend/tests-e2e/21-object-cost-depth.spec.ts docs/superpowers/specs/2026-09-14-object-cost-depth-design.md docs/superpowers/specs/2026-09-14-follow-ups-design.md
git commit -m "fix: include-contents is offered when every child is archived"
```
