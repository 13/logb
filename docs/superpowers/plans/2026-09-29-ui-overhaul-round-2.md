# UI overhaul, round 2 (dashboard and objects list) — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the dashboard in the new look: calm reminder cards, a one-row toolbar, object
cards with their tags inside and a non-stretching due badge, a responsive grid on desktop, a page
subtitle, and a dashboard "+ Log" that asks which object first. Release as 0.20.0.

**Architecture:** Tailwind utilities on the dashboard's own markup; no bits-ui on this screen.
The dashboard is the eagerly loaded chunk (`App.svelte`), and bits-ui's shared core is ~45 KB
gzipped, so pulling any bits-ui component in here would move that weight onto every first load.
The object picker is a native `<dialog>` (the app already uses them, e.g. `Reminders.svelte`),
the sort stays a native `<select>` styled with utilities, and Active/Archived stays two buttons
with `aria-pressed`.

**Tech Stack:** Svelte 5, Tailwind v4 (tokens in `frontend/src/app.tw.css`), Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-29-ui-overhaul-design.md` (Round 2, Rules for every round)

## Global Constraints

- No bits-ui (`$lib/components/ui/**`) imported by `Dashboard.svelte` or anything it imports; the
  eager chunk must not grow by more than 10 KB gzipped (round budget: +10 KB JS total, gzip -9).
- WCAG AA: text ≥ 4.5:1, focus rings and control outlines ≥ 3:1, both themes. Any new
  tinted-background/text pair is added to `frontend/tests/theme-contrast.test.ts` as a computed
  blend (see the existing `primary/15 over popover` test).
- Touch targets ≥ 44×44 px on mobile.
- Every new string in both `frontend/src/i18n/en.ts` and `de.ts`.
- A component moved to utilities deletes its own `<style>` rules for what it restyles; migrated
  markup sets `display`, `list-style` and heading sizes explicitly (app.css's revert block reaches
  it).
- Focus rings: `focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2
  focus-visible:outline-ring` on every new interactive element that is not a shadcn component
  (`outline-solid` is needed; `outline-none` elsewhere sets the style to none).
- e2e: no test deleted; tests that assert removed behaviour are changed to assert the new
  behaviour, and the change is named in the task report. Class-based locators on the dashboard
  become `getByTestId` / role locators.
- Screenshots before/after each task with `frontend/scripts/shots.mjs` (server:
  `rm -rf /tmp/logb-shots && LOGB_DATA_DIR=/tmp/logb-shots LOGB_PORT=8111 LOGB_BIND=127.0.0.1 LOGB_LOG=warn target/debug/logb &`,
  after `cd frontend && npm run build` and `cargo build`; kill it afterwards; the `/logb` process
  under uid 65532 is the user's own container — never touch it).
- Commits end with a `Co-Authored-By:` line naming the model that wrote them.
- No merge, tag or push: the round stops at a local release commit.

---

### Task 1: Reminder cards instead of banners

**Files:**
- Create: `frontend/src/lib/DashboardReminders.svelte`
- Modify: `frontend/src/routes/Dashboard.svelte` (the two `.banner` blocks and their `<style>`
  rules `.banner ul`, `.banner.soon`, `.banner.soon a`, `.snooze`), `frontend/src/i18n/en.ts`,
  `frontend/src/i18n/de.ts`, `frontend/tests/theme-contrast.test.ts`,
  `frontend/tests-e2e/23-tags.spec.ts`, and any other spec using `.banner` on the dashboard
- Test: `frontend/tests-e2e/36-dashboard.spec.ts` (new)

**Interfaces:**
- Produces: `<DashboardReminders due={Reminder[]} soon={Reminder[]} onsnooze={(r: Reminder) => void} />`.
  Each due reminder is an element with `data-testid="due-reminder"`, each upcoming one
  `data-testid="upcoming-reminder"`.

- [ ] **Step 1: Branch and capture "before"**

```bash
git switch -c ui-round-2 main
cd frontend && npm run build && cd .. && cargo build
rm -rf /tmp/logb-shots && LOGB_DATA_DIR=/tmp/logb-shots LOGB_PORT=8111 LOGB_BIND=127.0.0.1 LOGB_LOG=warn target/debug/logb &
sleep 2 && (cd frontend && node scripts/shots.mjs ../shots/r2-before); kill %1
cd frontend && for f in dist/assets/*.js dist/assets/*.css; do printf '%s %s\n' "$(gzip -9c "$f" | wc -c)" "$f"; done > ../.superpowers/sdd/bundle-r2-before.txt; cd ..
```

- [ ] **Step 2: Write the failing e2e test** — `frontend/tests-e2e/36-dashboard.spec.ts`:

```ts
import { expect, test } from '@playwright/test';
import { signInFresh } from './helpers';

/** The dashboard's reminder area: one card per due reminder with a real snooze button, calm rows
 *  for what is coming up, and no object tags on either (they belong to the object, not the
 *  reminder). */
test('due reminders are cards with a snooze button; upcoming ones are plain rows', async ({ page }) => {
  await signInFresh(page, '36-dashboard-reminders');
  const obj = await page.request.post('/api/objects', { data: { name: 'Dash reminder car', type: 'car', counter_unit: 'km', tags: ['family'] } });
  expect(obj.ok()).toBe(true);
  const id = (await obj.json()).id as number;
  const soonDate = new Date(Date.now() + 5 * 86_400_000).toISOString().slice(0, 10);
  for (const data of [{ title: 'Dash overdue', due_date: '2000-01-01' }, { title: 'Dash soon', due_date: soonDate }]) {
    expect((await page.request.post(`/api/objects/${id}/reminders`, { data })).ok()).toBe(true);
  }

  await page.goto('/');
  const due = page.getByTestId('due-reminder').filter({ hasText: 'Dash overdue' });
  await expect(due).toContainText('Dash reminder car');
  await expect(due.locator('.tag')).toHaveCount(0);
  await expect(due.locator('li')).toHaveCount(0);
  const snooze = due.getByRole('button', { name: /Snooze/ });
  const box = (await snooze.boundingBox())!;
  expect(box.height).toBeGreaterThanOrEqual(44);

  const soon = page.getByTestId('upcoming-reminder').filter({ hasText: 'Dash soon' });
  await expect(soon).toContainText(/in 5 days/);
  await expect(soon.locator('.tag')).toHaveCount(0);

  await due.getByRole('link', { name: /Dash overdue/ }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${id}\\?tab=reminders`));

  await page.goto('/');
  await page.getByTestId('due-reminder').filter({ hasText: 'Dash overdue' }).getByRole('button', { name: /Snooze/ }).click();
  await expect(page.getByTestId('due-reminder').filter({ hasText: 'Dash overdue' })).toHaveCount(0);
});
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cd frontend && npm run build && npx playwright test 36-dashboard`
Expected: FAIL — `getByTestId('due-reminder')` not found.

- [ ] **Step 4: i18n** — add to `en.ts` (beside the other `dash.*` keys):

```ts
  'dash.overdue-day': '1 day overdue',
  'dash.overdue-days': '{n} days overdue',
  'dash.due-today': 'due today',
```

and to `de.ts`:

```ts
  'dash.overdue-day': '1 Tag überfällig',
  'dash.overdue-days': '{n} Tage überfällig',
  'dash.due-today': 'heute fällig',
```

- [ ] **Step 5: `frontend/src/lib/DashboardReminders.svelte`**

```svelte
<script lang="ts">
  import { go } from './router';
  import { t } from '../i18n';
  import { fmtDate } from './format';
  import { dateFormat } from '../stores/date-format';
  import type { Reminder } from './types';

  let { due, soon, onsnooze }: { due: Reminder[]; soon: Reminder[]; onsnooze: (r: Reminder) => void } = $props();

  /** How late a due reminder is, when it is due by date. A counter-only reminder has no date to
   *  be late against, so it says nothing here; its card already says it is due. */
  function lateness(r: Reminder): string {
    if (r.days_until === null) return '';
    if (r.days_until === 0) return $t('dash.due-today');
    if (r.days_until === -1) return $t('dash.overdue-day');
    if (r.days_until < 0) return $t('dash.overdue-days', { n: -r.days_until });
    return '';
  }

  const open = (r: Reminder) => go(`/objects/${r.object_id}?tab=reminders`);
  const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring';
</script>

<!-- One card per due reminder: the thing to act on, with the action on it. Tinted, not filled --
     a solid red block took the whole first screen and made everything under it look secondary.
     No object tags: they describe the object, and on a reminder they read as the reminder's. -->
{#if due.length > 0}
  <section class="mb-4 flex flex-col gap-2" aria-label={due.length === 1 ? $t('dash.due-one') : $t('dash.due', { n: due.length })}>
    {#each due as r (r.id)}
      <div data-testid="due-reminder" class="flex items-center gap-3 rounded-lg border border-destructive/30 bg-destructive/10 p-3">
        <span class="size-2 shrink-0 rounded-full bg-destructive" aria-hidden="true"></span>
        <div class="min-w-0 flex-1">
          <a href={`/objects/${r.object_id}?tab=reminders`} class={`block truncate font-semibold text-foreground no-underline ${focus}`}
             onclick={(e) => { e.preventDefault(); open(r); }}>{r.title}</a>
          <p class="truncate text-sm text-muted-foreground">{r.object_name}{#if lateness(r)} · {lateness(r)}{/if}</p>
        </div>
        {#if r.kind === 'reading'}
          <!-- The whole job is one number, so it is one tap from here. -->
          <button data-slot="dash-action" class={`min-h-11 shrink-0 cursor-pointer rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground ${focus}`}
                  onclick={() => go(`/objects/${r.object_id}/reading`)}>{$t('reminder.record')}</button>
        {/if}
        <button data-slot="dash-action" class={`min-h-11 shrink-0 cursor-pointer rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground ${focus}`}
                onclick={() => onsnooze(r)}>{$t('reminder.snooze')}</button>
      </div>
    {/each}
  </section>
{/if}

{#if soon.length > 0}
  <section class="mb-4 rounded-lg border border-border bg-card p-3">
    <h2 class="m-0 mb-1 text-xs font-semibold tracking-wide text-muted-foreground uppercase">{$t('dash.upcoming')}</h2>
    {#each soon as r (r.id)}
      <div data-testid="upcoming-reminder" class="flex items-baseline justify-between gap-3 border-t border-border py-2 first-of-type:border-t-0">
        <a href={`/objects/${r.object_id}?tab=reminders`} class={`min-w-0 truncate text-foreground no-underline ${focus}`}
           onclick={(e) => { e.preventDefault(); open(r); }}><span class="font-medium">{r.title}</span> <span class="text-muted-foreground">· {r.object_name}</span></a>
        <span class="shrink-0 text-sm text-muted-foreground tabular-nums">
          {#if r.days_until !== null}{r.days_until === 1 ? $t('dash.in-day') : $t('dash.in-days', { n: r.days_until })}{/if}
          {#if r.counter_until !== null && r.counter_unit}{r.days_until !== null ? ' · ' : ''}{$t('dash.in-counter', { n: r.counter_until, unit: r.counter_unit })}{/if}
          {#if r.estimated_due_date} · {$t('dash.estimated', { date: fmtDate(r.estimated_due_date, $dateFormat) })}{/if}
        </span>
      </div>
    {/each}
  </section>
{/if}
```

(`data-slot` on the plain buttons exempts them from app.css's legacy button rules; the value is
only a marker.)

- [ ] **Step 6: Use it in `Dashboard.svelte`**

Replace both `{#if due.length > 0} <div class="banner">…{/if}` and
`{#if soon.length > 0} <div class="banner soon">…{/if}` blocks with:

```svelte
  <DashboardReminders {due} {soon} onsnooze={snooze} />
```

add `import DashboardReminders from '../lib/DashboardReminders.svelte';`, and delete the
`.banner ul`, `.banner.soon`, `.banner.soon a` and `.snooze` rules from its `<style>`. Remove the
now-unused imports (`TagChips` stays only if still used elsewhere in the file; `fmtDate` and
`dateFormat` move out if unused). `npm run check` reports unused imports as warnings — leave none.

- [ ] **Step 7: Contrast test for the tint** — in `frontend/tests/theme-contrast.test.ts`, next
  to the existing computed-blend tests, add for both themes: `destructive` on (destructive at 10%
  over card) ≥ 4.5:1, and `foreground` on the same blend ≥ 4.5:1, using the same blend helper the
  file already has.

- [ ] **Step 8: Update the dashboard part of `23-tags.spec.ts`**

The test "a reminder row shows its object's tags on the dashboard and the reminders tab" asserted
tags on the dashboard row. The spec removes them there (audit #3). Rename it to
"a reminder shows its object's tags on the reminders tab, not on the dashboard" and replace its
dashboard lines with:

```ts
  await page.goto('/');
  const row = page.getByTestId('due-reminder').filter({ hasText: 'Kette prüfen' });
  await expect(row).toBeVisible();
  // The tags describe the object; on the dashboard's reminder they read as the reminder's own.
  await expect(row.locator('.tag')).toHaveCount(0);
```

leaving the reminders-tab half unchanged. Search `tests-e2e/` for any other `.banner` use on the
dashboard (`grep -n "banner" tests-e2e/*.ts`) and move it to the test ids.

- [ ] **Step 9: Verify**

Run: `npm run check && npm test && npx playwright test 36-dashboard 23-tags 15-reading-reminders 32-calendar-reminders`
Expected: PASS. Then the full `npm run e2e`: PASS.
Capture `03-dashboard` into `../shots/r2-t1` (`node scripts/shots.mjs ../shots/r2-t1 03-dashboard`)
and look at mobile/desktop × light/dark next to `shots/r2-before`.

- [ ] **Step 10: Commit**

```bash
git add -A frontend
git commit -m "feat: reminder cards on the dashboard

Due reminders are tinted cards with a real snooze button; upcoming ones
are plain rows. Neither repeats the object's tags."
```

(plus the `Co-Authored-By:` line.)

### Task 2: Object cards

**Files:**
- Modify: `frontend/src/lib/ObjectCard.svelte` (markup and `<style>`), `frontend/src/i18n/en.ts`,
  `frontend/src/i18n/de.ts`, `frontend/tests/theme-contrast.test.ts`, the e2e specs that locate
  cards by class
- Test: `frontend/tests-e2e/36-dashboard.spec.ts`

**Interfaces:**
- Consumes: nothing new.
- Produces: `ObjectCard` keeps its props (`object`, `parentName`, `ontag`, `activeTag`). The card
  root has `data-testid="object-card"`; its main button's accessible name still begins with the
  object's name; the due badge has `data-testid="due-badge"`. The per-card quick-log "+" is gone
  (Task 4 adds the dashboard "+ Log").

- [ ] **Step 1: Write the failing e2e test** — append to `36-dashboard.spec.ts`:

```ts
test('an object card holds its tags, a compact due badge and a labelled last entry', async ({ page }) => {
  await signInFresh(page, '36-dashboard-card');
  const obj = await page.request.post('/api/objects', { data: { name: 'Card drill', type: 'tool', tags: ['garage'] } });
  const id = (await obj.json()).id as number;
  expect((await page.request.post(`/api/objects/${id}/activities`, { data: { date: '2025-06-10', category: 'other', title: 'Old entry' } })).ok()).toBe(true);
  expect((await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Card due', due_date: '2000-01-01' } })).ok()).toBe(true);

  await page.goto('/');
  const card = page.getByTestId('object-card').filter({ hasText: 'Card drill' });
  // Tags inside the card's box, not hanging below it.
  const cardBox = (await card.boundingBox())!;
  const tagBox = (await card.getByRole('button', { name: 'garage' }).boundingBox())!;
  expect(tagBox.y + tagBox.height).toBeLessThanOrEqual(cardBox.y + cardBox.height);
  // The badge is as wide as its text, not the rest of the row.
  const badge = card.getByTestId('due-badge');
  await expect(badge).toHaveText(/1 reminder due/);
  expect((await badge.boundingBox())!.width).toBeLessThan(cardBox.width / 2);
  // A last entry older than a month says what the date is.
  await expect(card).toContainText(/Last entry Jun 2025/);
  // No unlabelled quick-log square any more.
  await expect(card.getByRole('button', { name: /^Log$/ })).toHaveCount(0);
  // The tag filters; the rest of the card opens the object.
  await card.getByRole('button', { name: 'garage' }).click();
  await expect(page.getByTestId('tag-filter')).toContainText('garage');
  await card.getByRole('button', { name: /Card drill/ }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${id}$`));
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `npm run build && npx playwright test 36-dashboard -g "object card"`
Expected: FAIL — no `object-card` test id.

- [ ] **Step 3: i18n** — `en.ts`: `'dash.last-entry': 'Last entry {when}',`; `de.ts`:
  `'dash.last-entry': 'Letzter Eintrag {when}',`.

- [ ] **Step 4: Rewrite `ObjectCard.svelte`'s markup and delete its `<style>` block**

Keep the `<script>` (drop the `quickLogPath` import if nothing else uses it), and replace
everything below it with:

```svelte
<!-- The card is one box holding everything about the object. The whole box opens the object:
     the main button's ::after covers the card (`after:absolute after:inset-0`), so the name and
     facts are one big target and 26 e2e tests still find the object by that button's name. The
     tag chips are buttons of their own and cannot sit inside another button, so they are
     siblings raised above the stretched target (`relative z-10`). -->
<article data-testid="object-card" class="relative flex gap-3 rounded-lg border border-border bg-card p-3 shadow-xs transition-colors hover:border-input">
  {#if object.cover_file_id}
    <img class="size-12 shrink-0 rounded-md object-cover" src={fileUrl(object.cover_file_id, true)} alt="" loading="lazy" decoding="async" />
  {:else}
    <span class="grid size-12 shrink-0 place-items-center rounded-md bg-primary/15 text-brand-ink" aria-hidden="true">
      <Icon name={typeIcon(object.type, $customTypes)} size={22} />
    </span>
  {/if}
  <div class="flex min-w-0 flex-1 flex-col gap-1">
    <div class="flex items-start gap-2">
      <button data-slot="card-open"
              class="min-w-0 flex-1 cursor-pointer truncate text-left text-base font-semibold text-foreground after:absolute after:inset-0 after:rounded-lg after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-solid focus-visible:after:outline-offset-2 focus-visible:after:outline-ring"
              onclick={() => go(`/objects/${object.id}`)}>{object.name}</button>
      {#if object.stats.due_reminder_count > 0}
        <span data-testid="due-badge" class="shrink-0 rounded-full bg-destructive/10 px-2 py-0.5 text-xs font-semibold text-destructive">
          {object.stats.due_reminder_count === 1 ? $t('dash.due-one') : $t('dash.due', { n: object.stats.due_reminder_count })}
        </span>
      {/if}
      {#if object.archived_at}<span class="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('dash.archived')}</span>{/if}
      {#if object.pending}<span class="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('timeline.pending')}</span>{/if}
    </div>
    {#if parentName}<p class="m-0 truncate text-xs text-muted-foreground">{$t('search.in-parent', { name: parentName })}</p>{/if}
    <p class="m-0 text-sm text-muted-foreground tabular-nums">
      {typeLabel(object.type, $customTypes, $t, $typesLoaded)}
      {#if object.type === 'body' && object.stats.latest_weight_grams != null} · {formatWeight(object.stats.latest_weight_grams, object.weight_unit ?? 'kg', $locale)}{/if}
      {#if object.stats.current_counter !== null} · {counter(object.stats.current_counter, object.counter_unit, $locale)}{/if}
      <!-- A month is the unit people think in; rounded like the Info tab, since it is an average. -->
      {#if object.stats.counter_per_day_milli !== null && object.counter_unit} · {$t('insights.per-month', { amount: counter(Math.round(object.stats.counter_per_day_milli * 30.44 / 1000), object.counter_unit, $locale) })}{/if}
      {#if object.stats.total_cost_cents > 0} · {money(object.stats.total_cost_cents, $currency, $locale)}{/if}
      {#if object.stats.last_activity_date} · {$t('dash.last-entry', { when: lastActivityLabel(object.stats.last_activity_date, todayIso(), $locale) })}{/if}
    </p>
    {#if (object.tags ?? []).length > 0}
      <div class="relative z-10 mt-1 w-fit"><TagChips tags={object.tags} onselect={ontag} active={activeTag} /></div>
    {/if}
  </div>
</article>
```

Check `lastActivityLabel`'s output for "today"/"yesterday" reads right after "Last entry"
(`Intl.RelativeTimeFormat` with `numeric: 'auto'` gives "today", "yesterday", "3 days ago").

- [ ] **Step 5: Due badge contrast** — in `theme-contrast.test.ts`, the `destructive on
  destructive/10 over card` pair from Task 1 already covers this badge; add a comment saying so.

- [ ] **Step 6: Dashboard list container**

In `Dashboard.svelte` replace `<div class="list">` with
`<div class="grid grid-cols-1 gap-3">` (Task 3 adds the desktop columns). The `.list` class from
app.css is no longer used on this screen.

- [ ] **Step 7: Move the e2e specs off the old classes**

Run `npm run e2e 2>&1 | tail -60`. Replace, in the specs that fail:
- `.list .list-card`, `.list-card` → `page.getByTestId('object-card')`
- `.card-row svg` → `page.getByTestId('object-card').locator('svg')`
- `.chip.due` **on the dashboard** → `getByTestId('due-badge')` (the object page's
  Reminders-tab badge also uses `.chip.due`; leave those lines — round 3 moves them)
- `.card` locators that meant a dashboard card → `getByTestId('object-card')`; `.card` on other
  screens stays
- a `getByRole('button', { name: 'Log' })` (quick-log) click, if any → open the object and use
  `logEntry` from `./helpers`
Keep each assertion's meaning. Re-run until everything passes. List every changed line in the
report.

- [ ] **Step 8: Verify, look, commit**

Run: `npm run check && npm test && npm run e2e` — all PASS. Capture `03-dashboard` into
`../shots/r2-t2` and compare with `shots/r2-t1`: tags inside cards, compact badge, icon tile,
nothing overlapping.

```bash
git add -A frontend
git commit -m "feat: object cards hold their tags and a compact due badge"
```

### Task 3: Toolbar, tabs, subtitle, grid and a header "New object"

**Files:**
- Modify: `frontend/src/routes/Dashboard.svelte`, `frontend/src/lib/object-list.ts`,
  `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: `frontend/tests/object-list.test.ts`, `frontend/tests-e2e/36-dashboard.spec.ts`

**Interfaces:**
- Produces: `activeSpend(objects: MemObject[]): number` in `object-list.ts` — the sum of
  `stats.total_cost_cents` over the given objects (every depth: each object's own cost counts
  once), ignoring pending rows.
- The "New object" action moves from the floating button to the page header (`TopBar`'s
  `children`), on both viewports: icon plus text on desktop, icon only on a phone with the text
  kept for screen readers, so its accessible name is `+ New object` everywhere and the 37 e2e
  uses of `getByRole('button', { name: /New object/ })` keep working. The floating spot is taken
  by Task 4's "+ Log".

- [ ] **Step 1: Failing unit test** — append to `frontend/tests/object-list.test.ts` (reuse its
  existing object factory; if it has none, build a minimal `MemObject` literal with only the
  fields `activeSpend` reads, cast with `as MemObject`):

```ts
describe('activeSpend', () => {
  it('adds every object\'s own cost once, children included, and skips queued rows', () => {
    const rows = [
      obj({ id: 1, parent_id: null, stats: { total_cost_cents: 1000 } }),
      obj({ id: 2, parent_id: 1, stats: { total_cost_cents: 250 } }),
      obj({ id: -3, pending: true, stats: { total_cost_cents: 999 } }),
    ];
    expect(activeSpend(rows)).toBe(1250);
  });

  it('is zero for an empty list', () => {
    expect(activeSpend([])).toBe(0);
  });
});
```

Run: `npx vitest run tests/object-list.test.ts` — FAIL (`activeSpend` is not exported).

- [ ] **Step 2: Implement** — in `object-list.ts`:

```ts
/** What the listed objects have cost so far, for the dashboard's subtitle. Each object's own
 *  total counts once -- a child's costs are its own, not folded into its parent's -- and an
 *  object still waiting in the outbox has no server total yet. */
export function activeSpend(objects: MemObject[]): number {
  let sum = 0;
  for (const o of objects) if (!o.pending) sum += o.stats.total_cost_cents;
  return sum;
}
```

Run the test — PASS.

- [ ] **Step 3: i18n** — `en.ts`: `'dash.subtitle': '{n} active · {spend} spent',`,
  `'dash.subtitle-one': '1 active · {spend} spent',`; `de.ts`:
  `'dash.subtitle': '{n} aktiv · {spend} ausgegeben',`, `'dash.subtitle-one': '1 aktiv · {spend} ausgegeben',`.

- [ ] **Step 4: Failing e2e test** — append to `36-dashboard.spec.ts`:

```ts
test('the dashboard has a subtitle, one toolbar row, and a grid on wide screens', async ({ page }, info) => {
  await signInFresh(page, '36-dashboard-toolbar');
  for (const name of ['Grid one', 'Grid two', 'Grid three']) {
    const r = await page.request.post('/api/objects', { data: { name, type: 'tool' } });
    const id = (await r.json()).id as number;
    await page.request.post(`/api/objects/${id}/activities`, { data: { date: '2026-01-02', category: 'purchase', title: 'Bought', cost_cents: 1000 } });
  }
  await page.goto('/');
  await expect(page.getByRole('main').locator('header')).toContainText(/3 active · .*30\.00 spent/);
  await expect(page.getByRole('button', { name: '+ New object' })).toBeVisible();

  const search = page.getByLabel('Search objects');
  const sort = page.getByLabel('Sort');
  const tabs = page.getByRole('button', { name: /^Active/ });
  if (info.project.name === 'desktop') {
    // One row: search, sort and the Active/Archived switch share a line.
    const ys = await Promise.all([search, sort, tabs].map(async (l) => Math.round((await l.boundingBox())!.y)));
    expect(Math.max(...ys) - Math.min(...ys)).toBeLessThan(12);
    // Two columns at 1280 px.
    const cards = page.getByTestId('object-card');
    const [a, b] = await Promise.all([cards.nth(0).boundingBox(), cards.nth(1).boundingBox()]);
    expect(Math.round(a!.y)).toBe(Math.round(b!.y));
  }
  await expect(tabs).toHaveAttribute('aria-pressed', 'true');
});
```

Run: `npm run build && npx playwright test 36-dashboard -g "toolbar"` — FAIL.

- [ ] **Step 5: Dashboard markup**

In `Dashboard.svelte`:

1. Import `activeSpend` from `../lib/object-list`, `money` from `../lib/format` and `currency`
   from `../stores/session`, and derive:

```ts
  const subtitle = $derived(activeCount === 0 ? null
    : (activeCount === 1 ? $t('dash.subtitle-one', { spend: money(activeSpend(active), $currency, $locale) })
      : $t('dash.subtitle', { n: activeCount, spend: money(activeSpend(active), $currency, $locale) })));
```

2. The header: `<TopBar title={$t('dash.title')} {subtitle}>` with, as its child,

```svelte
    <!-- A phone's header has room for an icon, not for the words: the text stays for screen
         readers (and for the accessible name the e2e suite finds it by). -->
    <button data-slot="dash-action"
            class="inline-flex min-h-11 min-w-11 shrink-0 cursor-pointer items-center justify-center gap-1 rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring"
            onclick={() => go('/objects/new')}><span aria-hidden="true">+</span> <span class="max-desk:sr-only">{$t('dash.new')}</span></button>
```

   and close `</TopBar>`. Its accessible name must be "+ New object" on both viewports — but the
   `+` is `aria-hidden`, so set `aria-label={`+ ${$t('dash.new')}`}` on the button to make the
   name explicit, and check it in the e2e test below. Delete the old floating `+ New object`
   button block (Task 4 puts "+ Log" in its place).

3. Replace the `<nav class="tabs">…</nav>` and the `<div class="controls">…</div>` with one
   toolbar placed where `.controls` was, but rendered whenever the list is (also when the list is
   empty after filtering). Structure:

```svelte
    <div class="mb-3 flex flex-wrap items-center gap-2">
      <input type="search" data-slot="dash-search" aria-label={$t('dash.search')} placeholder={$t('dash.search')} bind:value={query}
             class="h-11 min-w-48 flex-1 rounded-md border border-input bg-card px-3 text-base text-foreground placeholder:text-muted-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring" />
      <label class="flex items-center gap-2 text-sm text-muted-foreground">
        <span>{$t('dash.sort')}</span>
        <select data-slot="dash-sort" value={sort} onchange={(e) => setSort(parseSort((e.currentTarget as HTMLSelectElement).value) ?? 'name')}
                class="h-11 rounded-md border border-input bg-card px-3 text-base text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">
          {#each SORT_KEYS as key (key)}<option value={key}>{$t(`dash.sort-${key}`)}</option>{/each}
        </select>
      </label>
      <div class="flex rounded-md bg-muted p-1" role="group" aria-label={$t('dash.title')}>
        {#each [['active', $t('dash.tab-active'), activeCount], ['archived', $t('dash.tab-archived'), archivedKnown ? archived.length : '']] as [key, label, count] (key)}
          <button data-slot="dash-tab" aria-pressed={tab === key} onclick={() => (tab = key as ListTab)}
                  class={['min-h-9 cursor-pointer rounded px-3 text-sm font-medium focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring',
                          tab === key ? 'bg-card text-foreground shadow-xs' : 'text-muted-foreground hover:text-foreground']}>
            {label} <span class="ml-1 tabular-nums text-muted-foreground">{count}</span>
          </button>
        {/each}
      </div>
    </div>
```

   Keep the toolbar visible in the loading and empty-archive states so the switch is always
   reachable (the old tabs were); the first-run empty state (`nothingYet`) hides it. Keep the
   `select` native: the e2e `selectOption` calls depend on it, and a bits-ui select would put the
   library into the eager chunk. The segmented buttons are 36 px tall inside a 44 px group — the
   whole group row is the touch target height; if the 44 px rule is read per button, make them
   `min-h-11` and drop the group's padding instead (check on the mobile screenshot which reads
   better, and record the choice).

4. The tag filter: `<div class="tag-filter">` → `<div data-testid="tag-filter" class="mb-3 flex flex-wrap items-center gap-2">`;
   its clear button gets the same outline-button classes as the header button (without the
   `hidden desk:inline-flex`, with `inline-flex`).

5. The list: `grid grid-cols-1 gap-3` → `grid grid-cols-1 gap-3 min-[1024px]:grid-cols-2 min-[1440px]:grid-cols-3`.

6. Delete the now-unused rules from the `<style>` block: `.controls`, `.controls input[type='search']`,
   `.sort`, `.tag-filter`, `.tag-filter button`, `.tabs .count`. If the block ends up empty,
   delete it.

7. Update `22-objects-list` and any spec that located `.tag-filter` to `getByTestId('tag-filter')`.

- [ ] **Step 6: Verify, look, commit**

Run: `npm run check && npm test && npm run e2e` — PASS. Capture `03-dashboard` into
`../shots/r2-t3` and compare: one toolbar row on desktop, wrapping sensibly on mobile, subtitle
under the title, 2 columns at 1440 px (the capture's desktop width) — and 3 columns if `main`'s
width allows it; if `main`'s 1100 px cap makes 3 columns too narrow (under ~300 px per card),
drop the `min-[1440px]` step and record why.

```bash
git add -A frontend
git commit -m "feat: dashboard toolbar, subtitle, grid, and New object in the header"
```

### Task 4: "+ Log" on the dashboard, with an object picker

**Files:**
- Create: `frontend/src/lib/LogPicker.svelte`
- Modify: `frontend/src/routes/Dashboard.svelte`, `frontend/src/lib/object-list.ts`,
  `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: `frontend/tests/object-list.test.ts`, `frontend/tests-e2e/36-dashboard.spec.ts`

**Interfaces:**
- Consumes: `quickLogPath(object)` from `frontend/src/lib/weight.ts` (the destination the old
  per-card "+" had: weight for a body, fill/usage for a resource, reading for a counter, else a
  new activity).
- Produces:
  - `pickerOrder(objects: MemObject[]): MemObject[]` in `object-list.ts` — active,
    non-pending objects, most recent `last_activity_date` first (none last), then by name.
  - `<LogPicker objects={MemObject[]} onpick={(o: MemObject) => void} />`
    rendering the floating "+ Log" button and a native `<dialog>`.

- [ ] **Step 1: Failing unit test** — `object-list.test.ts`:

```ts
describe('pickerOrder', () => {
  it('puts the most recently used objects first and leaves out queued ones', () => {
    const rows = [
      obj({ id: 1, name: 'Beta', stats: { last_activity_date: '2026-01-01' } }),
      obj({ id: 2, name: 'Alpha', stats: { last_activity_date: null } }),
      obj({ id: 3, name: 'Gamma', stats: { last_activity_date: '2026-09-01' } }),
      obj({ id: -4, name: 'Queued', pending: true, stats: { last_activity_date: '2026-09-02' } }),
    ];
    expect(pickerOrder(rows).map((o) => o.name)).toEqual(['Gamma', 'Beta', 'Alpha']);
  });
});
```

Run — FAIL. Implement in `object-list.ts`:

```ts
/** The dashboard's "+ Log" list: what was used last is most likely what is logged next. An
 *  object still in the outbox has no id to log against yet, so it is left out. */
export function pickerOrder(objects: MemObject[]): MemObject[] {
  return objects
    .filter((o) => !o.pending)
    .toSorted((a, b) => (b.stats.last_activity_date ?? '').localeCompare(a.stats.last_activity_date ?? '')
      || collator().compare(a.name, b.name));
}
```

(Use the same `collator` import the file already has; if its call signature differs, match it.)
Run — PASS.

- [ ] **Step 2: i18n** — `en.ts`: `'dash.pick-title': 'Log for which object?'`,
  `'dash.pick-search': 'Find an object'`, `'dash.pick-none': 'No object matches.'`; `de.ts`:
  `'dash.pick-title': 'Für welches Objekt erfassen?'`, `'dash.pick-search': 'Objekt suchen'`,
  `'dash.pick-none': 'Kein Objekt passt.'`.

- [ ] **Step 3: Failing e2e test** — append to `36-dashboard.spec.ts`:

```ts
test('"+ Log" on the dashboard asks for the object, then opens its quick entry', async ({ page }) => {
  await signInFresh(page, '36-dashboard-log');
  const car = (await (await page.request.post('/api/objects', { data: { name: 'Picker car', type: 'car', counter_unit: 'km' } })).json()).id as number;
  await page.request.post('/api/objects', { data: { name: 'Picker drill', type: 'tool' } });
  await page.goto('/');

  await page.getByRole('button', { name: /^\+ Log$/ }).click();
  const dialog = page.getByRole('dialog', { name: /Log for which object/ });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByLabel('Find an object')).toBeFocused();
  await dialog.getByLabel('Find an object').fill('car');
  await expect(dialog.getByRole('button', { name: /Picker drill/ })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();

  await page.getByRole('button', { name: /^\+ Log$/ }).click();
  await dialog.getByRole('button', { name: /Picker car/ }).click();
  // A car with a counter goes to its reading, as the old per-card "+" did.
  await expect(page).toHaveURL(new RegExp(`/objects/${car}/reading$`));
});
```

Run — FAIL.

- [ ] **Step 4: `frontend/src/lib/LogPicker.svelte`**

```svelte
<script lang="ts">
  import { t } from '../i18n';
  import { customTypes, typeIcon } from './type-registry';
  import Icon from './Icon.svelte';
  import type { MemObject } from './types';

  let { objects, onpick }: { objects: MemObject[]; onpick: (o: MemObject) => void } = $props();

  let dialog = $state<HTMLDialogElement | null>(null);
  let search = $state<HTMLInputElement | null>(null);
  let query = $state('');
  const fold = (s: string) => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
  const shown = $derived(query.trim() === '' ? objects : objects.filter((o) => fold(o.name).includes(fold(query.trim()))));

  function open() {
    query = '';
    dialog?.showModal();
    // A native dialog focuses its first focusable element, which is the search box, but only
    // after it opens; asking for it explicitly keeps that true if the order ever changes.
    search?.focus();
  }
  function pick(o: MemObject) { dialog?.close(); onpick(o); }
  const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring';
</script>

<!-- A native dialog, not a bits-ui one: this is on the dashboard, the screen every start draws
     first, and bits-ui's shared core would ride along into that chunk. -->
<button data-slot="dash-fab" onclick={open}
        class={`fab h-12 cursor-pointer rounded-full bg-primary px-5 text-base font-semibold text-primary-foreground shadow-lg ${focus}`}>+ {$t('dash.log')}</button>

<dialog bind:this={dialog} aria-labelledby="log-picker-title"
        class="m-auto w-[min(92vw,28rem)] rounded-xl border border-border bg-popover p-0 text-popover-foreground shadow-xl backdrop:bg-black/45">
  <div class="flex flex-col gap-3 p-4">
    <h2 id="log-picker-title" class="m-0 text-lg font-semibold">{$t('dash.pick-title')}</h2>
    <input bind:this={search} bind:value={query} type="search" data-slot="dash-search" aria-label={$t('dash.pick-search')} placeholder={$t('dash.pick-search')}
           class={`h-11 rounded-md border border-input bg-card px-3 text-base text-foreground placeholder:text-muted-foreground ${focus}`} />
    <div class="flex max-h-[50vh] flex-col overflow-y-auto">
      {#each shown as o (o.id)}
        <button data-slot="dash-pick" onclick={() => pick(o)}
                class={`flex min-h-11 cursor-pointer items-center gap-3 rounded-md px-2 text-left text-foreground hover:bg-accent ${focus}`}>
          <Icon name={typeIcon(o.type, $customTypes)} size={18} />
          <span class="truncate">{o.name}</span>
        </button>
      {:else}
        <p class="m-0 py-2 text-sm text-muted-foreground">{$t('dash.pick-none')}</p>
      {/each}
    </div>
    <div class="flex justify-end border-t border-border pt-3">
      <button data-slot="dash-action" onclick={() => dialog?.close()}
              class={`min-h-11 cursor-pointer rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground ${focus}`}>{$t('form.cancel')}</button>
    </div>
  </div>
</dialog>
```

Check the cancel key: `grep -n "'form.cancel'\|'common.cancel'\|cancel'" src/i18n/en.ts` and use
the key the forms already use for "Cancel". The `fab` class keeps app.css's floating position
(both breakpoints, including the desktop gutter maths).

- [ ] **Step 5: Use it in `Dashboard.svelte`**

Where Task 3 removed the floating `+ New object` button, add:

```svelte
  {#if !nothingYet}
    <LogPicker objects={pickerOrder(active)} onpick={(o) => go(quickLogPath(o))} />
  {/if}
```

importing `LogPicker`, `pickerOrder` and `quickLogPath` (from `../lib/weight`). The first-run empty state keeps its own "+ New object" button.

- [ ] **Step 6: Verify**

Run: `npm run check && npm test && npm run e2e`. Expected PASS on both projects. Open the picker on a phone and a desktop
screenshot (a short Playwright snippet like Task 7's of round 1), light and dark: search box
focused ring visible, list readable, backdrop dims the page.

- [ ] **Step 7: Commit**

```bash
git add -A frontend
git commit -m "feat: \"+ Log\" on the dashboard asks for the object first"
```

### Task 5: Budget check and release 0.20.0

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `frontend/package.json`, `frontend/package-lock.json`,
  `docs/openapi.json`, `docs/upgrading.md`

- [ ] **Step 1: Measure** — build and compare with `.superpowers/sdd/bundle-r2-before.txt`
  (same `gzip -9` loop as round 1). Expected: JS delta ≤ 10240 in total, and the eager entry
  chunk (`index-*.js`) grows by at most that too; `grep -l "bits-ui\|floating-ui" frontend/dist/assets/index-*.js`
  prints nothing. If over, report the numbers and the biggest contributors instead of bumping.

- [ ] **Step 2: Bump to 0.20.0** in the six files (as round 1's Task 8 did, from 0.19.0), and
  add above `## 0.19.0` in `docs/upgrading.md`:

```markdown
## 0.20.0: new look, the dashboard

**The dashboard is redesigned.** Due reminders are cards with a snooze button, what is coming
up is a short list, and each object's tags sit inside its card. Search, sort and the
Active/Archived switch share one row; on a wide screen the objects fill two columns.

**"+ Log" on the dashboard asks which object**, then opens the same quick entry the small "+" on
each card used to: a reading for something with a counter, a fill for a tank, a weight for a
person. The per-card "+" is gone, and "New object" moved to the page header.

No migration.
```

- [ ] **Step 3: Verify and commit**

```bash
cargo clippy --all-targets -- -D warnings
cargo test --test it openapi::
(cd frontend && npm run check && npm test && npm run e2e)
git add Cargo.toml Cargo.lock frontend/package.json frontend/package-lock.json docs/openapi.json docs/upgrading.md
git commit -m "chore: release 0.20.0"
```

Report bundle deltas, test counts, and before/after dashboard screenshots
(`shots/r2-before` vs the last set). Stop; merge/tag/push is the user's decision.
