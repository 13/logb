# UI overhaul, round 3 (object detail): implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the object page in the new look. From 1024 px it has two panes: a sticky summary
on the left and tabbed history on the right, with "+ Log" and Edit in the page header. On a phone
the photo, a scrolling strip of figures and anything due sit above the tabs, which stay on one
row, and Info holds the rest. Timeline entries get a category icon, the amount on the right and
their tags inside. A hand-written SVG `Chart` hides empty months, and the Energy figures only show
for objects that charge. Release as 0.21.0.

**Architecture:** Tailwind utilities and shadcn/bits-ui components on the object page and the
components it renders. `ObjectDetail` is a lazily loaded chunk that already carries bits-ui (the
"+ Log" dropdown), so bits-ui Tabs costs little here. The summary markup exists once and CSS moves
it (above the tabs on a phone, the left pane on desktop). The rest of the old Info tab becomes one
component, `ObjectDetails`, which is rendered in exactly one place. A single `MediaQuery` in
`ObjectDetail.svelte` decides where: the left pane from 1024 px, the Info tab below that. See
Decisions 1 and 2.

**Tech Stack:** Svelte 5, Tailwind v4 (tokens in `frontend/src/app.tw.css`), bits-ui 2, @lucide/svelte, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-29-ui-overhaul-design.md` (Round 3, Rules for every round)

## Decisions (refinements of the spec, for the controller to relay)

1. **Breakpoint `wide:` = 1024 px.** Added to `app.tw.css` as `--breakpoint-wide: 1024px` (px, range
   syntax). The script side uses the identical query text. Tailwind's `lg:` is `64rem` and would
   drift away from a px query if the root font size changed.
2. **One summary, one details component, no duplicated markup.** The summary part (photo,
   type · since · tags, figures, due reminders) is one DOM node, and CSS places it in both layouts.
   The rest of the old Info content (`ObjectDetails`) is one Svelte snippet rendered in one place
   at a time. `new MediaQuery('(width >= 1024px)')` (`svelte/reactivity`) picks the left pane or
   the Info tab.
   *Why a JS query here, when the app otherwise avoids them (AppNav's comment):* this switch is
   structural. It decides whether an Info tab exists, which container owns the details, and
   whether Contents, Last done and Trips load now or when Info opens. The CSS-only alternatives
   are worse:
   - Rendering the details twice duplicates the parts that fetch their own data, duplicates test
     ids, and puts hidden copies in the accessibility tree.
   - `display: contents` with `order` makes the focus order differ from the visual order on
     phones (WCAG 2.4.3).
   Svelte's `MediaQuery` reads `matchMedia` synchronously on mount (the page is client-only) and
   follows resizes, so AppNav's objection ("wrong on first paint") does not apply. Everything
   purely visual stays in CSS. This is the only JS breakpoint.
3. **The name is not repeated in the left pane.** The page header's `<h1>` (the focus target
   after navigation, next to the back button) shows it at every width. The pane starts with the
   photo. The spec lists "name" in the pane.
4. **"+ Log" and Edit sit in the page header (`TopBar`) from 1024 px.** The header spans both
   panes. Round 1's spec text says "moves the button into the desktop page header". The header
   "+ Log" shows on every tab except an empty, unfiltered timeline, because that empty state
   already shows the same buttons. Below 1024 px "+ Log" keeps floating on the Timeline tab.
5. **On desktop the left pane holds everything the Info tab held.** There is no Info tab there,
   so the rest needs a home. The order is:
   - the spec's parts: photo, type · since · tags, figures, due reminders, spend per month;
   - then description and purchase price, Last done, Trips, Energy, Contents, the Cost breakdown
     and the CSV import;
   - then "New object inside" and "Export this object" close the pane.
   The pane is sticky and scrolls on its own (`max-height` = viewport minus header). A sticky
   element taller than the viewport would hide its end until the timeline ran out.
6. **`?tab=info` at desktop width opens the Timeline.** The details are already on screen. The
   address is normalised to the plain URL.
7. **Tabs become real ARIA tabs** (bits-ui Tabs: `role="tab"`, `aria-selected`, arrow keys).
   - e2e locators change from `getByRole('button', { name: 'Info' })` to `getByRole('tab', …)`
     and from `toHaveClass(/active/)` to `aria-selected`. Opening Info goes through a new
     `openInfo(page)` helper, which does nothing at desktop width.
   - The four thin wrapper files under `components/ui/tabs/` are written by hand, not by the CLI.
     The CLI's version is a segmented control, and the app keeps underline tabs. Written by hand,
     the wrappers ship with full-strength focus rings from the start.
8. **Chart.**
   - It drops every empty month (zero, or a usage the readings cannot measure). Each remaining
     bar keeps its own label, so a gap shows in the labels.
   - Axis labels show the month only ("Sep"). Past six bars every other label is drawn, counted
     from the newest. The full label ("Sep 26") is in the bar's tooltip and in a screen-reader
     table under the chart.
   - Bars are `brand-ink`. It meets 3:1 non-text contrast; the amber fill is 2.1:1.
   - Four time series use it: spend, usage and trip distance per month, and consumption per fill.
     "Per year" and "Per category" stay `BarList`: they are categories, not months, and
     `BarList` is shared with Statistics, which round 5 redoes.
9. **Figures.** Up to four figures:
   - total cost (for a body, the latest weight instead);
   - counter;
   - usage per month;
   - consumption (when the Cost data has it).
   Only the figures that apply are shown. "Activities" fills a row that has fewer than four.
   "Owned since" is no longer a figure, because the meta line says "since Apr 2022".
10. **Energy section only for kWh objects.** Its figures are about charging: distance per charge,
    per kWh, charge due. For a liquid fuel, the Cost section's consumption and fuel cost per km
    already say it. The audit found "Distance per charge" on a diesel car. Trip cost estimates
    still use the energy rate for every fuel.
11. **Category icons come from `@lucide/svelte`.** It is already a dependency, used by the
    generated dropdown. The icons load only in the lazy object chunk. `Icon.svelte` (eager) is
    unchanged, so nothing in the icon set is replaced.
12. **The Reminders and Documents tabs move to utilities too.** They are part of the object page,
    and `app.css` goes in round 5. Their `.card`, `.chip.due` and `.thumb-grid` locators move to
    test ids. Their dialogs and the Reminders tab's floating "+ New reminder" keep `app.css`
    (`dialog`, `.fab`) until round 4/5.
13. **Insights data is fetched once, by `ObjectDetail`.** The summary needs its consumption and
    the pane needs its spend chart. On a phone that is one extra GET per object view, which
    before happened only when Info opened.
14. **WeightHistory (body) stays as it is**, at the top of the right-hand column.
15. **A due reminder in the summary opens the Reminders tab.** It has no snooze button there; the
    tab has the actions.

No open question needs the user.

## Global Constraints

- **Chunk boundary.**
  - Only the lazy object chunk may import `$lib/components/ui/**`, `@lucide/svelte` or
    `svelte/reactivity`: `ObjectDetail.svelte` and what it imports.
  - Nothing that `App.svelte` or `Dashboard.svelte` pulls in eagerly may import them.
  - The entry chunk (`index-*.js`) may grow by at most 200 bytes gzip. That allowance covers the
    `lateness` helper moving to its own module.
  - Round budget: +10 KB gzip JS in total, measured with `gzip -9` as in round 2.
- WCAG AA: text ≥ 4.5:1, focus rings, control outlines and chart bars ≥ 3:1, in both themes.
  Every new tinted-background/text pair gets a computed-blend test in
  `frontend/tests/theme-contrast.test.ts`, like the existing `blend(...)` tests. Mind the
  lesson: light `brand-ink` on `primary/15` over card fails at 4.47; `/10` passes, so tiles use
  `bg-primary/10`.
- Touch targets ≥ 44×44 px on mobile. For visually smaller chips, extend the hit area with a
  `::before` that stays inside the scroller's padding.
- Every new string goes into both `frontend/src/i18n/en.ts` and `de.ts`.
- A migrated component deletes its own `<style>` rules for what it restyles. Scoped styles are
  unlayered and beat utilities.
- app.css's revert block reaches new markup:
  - set `display`, `list-style` (`list-none`) and heading sizes explicitly;
  - give `<ul>` lists `role="list"`, because `list-none` drops list semantics in Safari;
  - give `<p>` and headings an explicit margin (`m-0` …).
- Plain `<button>`s in new markup carry a `data-slot="…"` marker (any value). It exempts them
  from app.css's legacy `button` rules. shadcn components already carry one.
- Focus rings on every new interactive element that is not a shadcn component:
  `focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring`.
  `outline-solid` is required. Inside a horizontal scroller, either pad the scroller by 4 px (the
  ring's bleed) or use `-outline-offset-2`.
- Clickable cards use the stretched-link pattern from `ObjectCard.svelte` and
  `DashboardReminders.svelte`: the title's `::after` covers the card, and chips or buttons inside
  sit above it (`relative z-10`).
- Layout switches use `wide:` / `max-wide:` (1024 px). The one JS query is in `ObjectDetail.svelte`
  (Decision 2); no other file reads a breakpoint in script.
- e2e:
  - No test is deleted. A test that asserts removed behaviour changes to assert the new
    behaviour, and the change is named in the task report.
  - Class locators on the object page become test ids or role locators.
  - The test ids this round introduces: `tab-due-badge`, `object-cover`, `object-type`,
    `figures`, `figure-<key>`, `summary-due-reminder`, `insights-spend`, `insights-usage`,
    `chart-bar`, `category-chips`, `timeline-filter`, `timeline-entry`, `timeline-reading`,
    `timeline-fold`, `entry-thumbs`, `last-done-row`, `reminder-card`, `reminder-status`,
    `reminder-done`, `document`.
  - `.tag` (TagChips) is not migrated this round, and its locators stay.
- Screenshots before and after each task with `frontend/scripts/shots.mjs`:
  1. Build first: `cd frontend && npm run build`, then `cargo build`.
  2. Start the server:
     `rm -rf /tmp/logb-shots && LOGB_DATA_DIR=/tmp/logb-shots LOGB_PORT=8111 LOGB_BIND=127.0.0.1 LOGB_LOG=warn target/debug/logb &`
  3. Capture: `node scripts/shots.mjs ../shots/r3-tN object`. The filter `object` captures
     05/06/07 and 11-object-new.
  4. Kill the server afterwards. The `/logb` process under uid 65532 is the user's own container;
     never touch it.
- Commits end with a `Co-Authored-By:` line naming the model that wrote them.
- No merge, tag or push: the round stops at a local release commit.

---

### Task 1: Real tabs that never wrap, with the due count inline

**Files:**
- Create: `frontend/src/lib/components/ui/tabs/index.ts`, `tabs.svelte`, `tabs-list.svelte`,
  `tabs-trigger.svelte`, `tabs-content.svelte` (all in `frontend/src/lib/components/ui/tabs/`)
- Modify: `frontend/src/routes/ObjectDetail.svelte`, `frontend/src/app.css`,
  `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`, `frontend/tests-e2e/helpers.ts`,
  `frontend/tests-e2e/02-lifecycle.spec.ts`, `11-controls.spec.ts`, `12-object-hierarchy.spec.ts`,
  `15-reading-reminders.spec.ts`, `18-templates.spec.ts`, `21-object-cost-depth.spec.ts`,
  `23-tags.spec.ts`, `24-own-types.spec.ts`, `27-last-done.spec.ts`
- Test: `frontend/tests-e2e/37-object-detail.spec.ts` (new)

**Interfaces:**
- Produces:
  - `import * as Tabs from '$lib/components/ui/tabs/index.js'` provides `Tabs.Root`,
    `Tabs.List`, `Tabs.Trigger` and `Tabs.Content` (bits-ui props).
  - The Reminders trigger holds `data-testid="tab-due-badge"`, and its accessible name is
    "Reminders (N due)".
  - `openInfo(page)` in `tests-e2e/helpers.ts` clicks the Info tab where one exists.

- [ ] **Step 1: Branch and capture "before"**

```bash
git switch -c ui-round-3 main
cd frontend && npm run build && cd .. && cargo build
rm -rf /tmp/logb-shots && LOGB_DATA_DIR=/tmp/logb-shots LOGB_PORT=8111 LOGB_BIND=127.0.0.1 LOGB_LOG=warn target/debug/logb &
sleep 2 && (cd frontend && node scripts/shots.mjs ../shots/r3-before object); kill %1
cd frontend && for f in dist/assets/*.js dist/assets/*.css; do printf '%s %s\n' "$(gzip -9c "$f" | wc -c)" "$f"; done > ../.superpowers/sdd/bundle-r3-before.txt; cd ..
```

- [ ] **Step 2: Write the failing e2e test.** Create `frontend/tests-e2e/37-object-detail.spec.ts`:

```ts
import { expect, test, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

/** The object page as round 3 of the UI overhaul left it: tabs, summary, panes, timeline, charts.
 *  Seeds through the API so each test drives only the screen it is about. */
async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id as number;
}

test('the tabs stay on one row and the due count sits inside the Reminders tab', async ({ page }) => {
  await signInFresh(page, '37-tabs');
  const id = await object(page, { name: 'Tabs car', type: 'car', counter_unit: 'km' });
  expect((await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Tabs overdue', due_date: '2000-01-01' } })).ok()).toBe(true);
  await page.goto(`/objects/${id}`);

  const tabs = page.getByRole('tab');
  await expect(tabs.first()).toBeVisible();
  const boxes = await Promise.all((await tabs.all()).map((tab) => tab.boundingBox()));
  const ys = boxes.map((b) => Math.round(b!.y));
  // One row: the audit's red "1" dropped onto a second line under "Reminders".
  expect(Math.max(...ys) - Math.min(...ys)).toBeLessThanOrEqual(1);
  for (const b of boxes) expect(b!.height).toBeGreaterThanOrEqual(44);

  const reminders = page.getByRole('tab', { name: /^Reminders/ });
  await expect(reminders).toHaveAccessibleName('Reminders (1 due)');
  const tab = (await reminders.boundingBox())!;
  const badge = (await reminders.getByTestId('tab-due-badge').boundingBox())!;
  expect(badge.y).toBeGreaterThanOrEqual(tab.y);
  expect(badge.y + badge.height).toBeLessThanOrEqual(tab.y + tab.height);
  expect(tab.height).toBeLessThan(56);

  // A tab list moves with the arrow keys, and the address follows the tab.
  await page.getByRole('tab', { name: 'Timeline', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab', { name: 'Documents', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page).toHaveURL(new RegExp(`/objects/${id}\\?tab=documents$`));
});
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cd frontend && npm run build && npx playwright test 37-object-detail`
Expected: FAIL. `getByRole('tab')` finds nothing.

- [ ] **Step 4: The tab components**

`frontend/src/lib/components/ui/tabs/tabs.svelte`:

```svelte
<script lang="ts">
	import { Tabs as TabsPrimitive } from 'bits-ui';
	import { cn } from '$lib/utils.js';

	let { ref = $bindable(null), value = $bindable(''), class: className, ...restProps }: TabsPrimitive.RootProps = $props();
</script>

<TabsPrimitive.Root bind:ref bind:value data-slot="tabs" class={cn('flex flex-col', className)} {...restProps} />
```

`frontend/src/lib/components/ui/tabs/tabs-list.svelte`:

```svelte
<script lang="ts">
	import { Tabs as TabsPrimitive } from 'bits-ui';
	import { cn } from '$lib/utils.js';

	let { ref = $bindable(null), class: className, ...restProps }: TabsPrimitive.ListProps = $props();
</script>

<!-- One row that scrolls sideways rather than wrapping: a wrapped tab row reads as two rows of
     choices, and a badge pushed onto its own line (the audit's red "1" under "Reminders") reads
     as a control of its own. -->
<TabsPrimitive.List
	bind:ref
	data-slot="tabs-list"
	class={cn('flex w-full overflow-x-auto border-b border-border [scrollbar-width:none] [&::-webkit-scrollbar]:hidden', className)}
	{...restProps}
/>
```

`frontend/src/lib/components/ui/tabs/tabs-trigger.svelte`:

```svelte
<script lang="ts">
	import { Tabs as TabsPrimitive } from 'bits-ui';
	import { cn } from '$lib/utils.js';

	let { ref = $bindable(null), class: className, ...restProps }: TabsPrimitive.TriggerProps = $props();
</script>

<!-- The ring is drawn inside the tab (-outline-offset-2): the list scrolls, and a scroller clips
     whatever is painted past its edge. -->
<TabsPrimitive.Trigger
	bind:ref
	data-slot="tabs-trigger"
	class={cn(
		'inline-flex min-h-11 shrink-0 cursor-pointer items-center gap-1.5 whitespace-nowrap border-b-2 border-transparent px-3 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring disabled:pointer-events-none disabled:opacity-50 data-[state=active]:border-brand-ink data-[state=active]:font-semibold data-[state=active]:text-brand-ink',
		className
	)}
	{...restProps}
/>
```

`frontend/src/lib/components/ui/tabs/tabs-content.svelte`:

```svelte
<script lang="ts">
	import { Tabs as TabsPrimitive } from 'bits-ui';
	import { cn } from '$lib/utils.js';

	let { ref = $bindable(null), class: className, ...restProps }: TabsPrimitive.ContentProps = $props();
</script>

<TabsPrimitive.Content
	bind:ref
	data-slot="tabs-content"
	class={cn('pt-3 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring', className)}
	{...restProps}
/>
```

`frontend/src/lib/components/ui/tabs/index.ts`:

```ts
import Root from './tabs.svelte';
import Content from './tabs-content.svelte';
import List from './tabs-list.svelte';
import Trigger from './tabs-trigger.svelte';

export { Root, Content, List, Trigger, Root as Tabs, Content as TabsContent, List as TabsList, Trigger as TabsTrigger };
```

(bits-ui renders every `Tabs.Content` and hides the inactive ones with `hidden`. The panels below
therefore keep their own `{#if}`, so Documents and Reminders still load only when opened.)

- [ ] **Step 5: i18n.** In `en.ts`, after `'tab.info': 'Info',` add:

```ts
  'tab.reminders-due': '({n} due)',
  'object.sections': 'Sections',
```

In `de.ts`, after `'tab.info'`:

```ts
  'tab.reminders-due': '({n} fällig)',
  'object.sections': 'Bereiche',
```

- [ ] **Step 6: Use them in `ObjectDetail.svelte`**

1. Add the import `import * as Tabs from '$lib/components/ui/tabs/index.js';`.
2. Replace everything from `<nav class="tabs">` through the `{/if}` that closes the tab panels
   (the one just before `{:else if !error}`) with the block below. The Info panel's content is
   unchanged; it moves inside `Tabs.Content`:

```svelte
    <Tabs.Root value={tab} onValueChange={(v) => setTab(v as Tab)}>
      <Tabs.List aria-label={$t('object.sections')}>
        <Tabs.Trigger value="timeline">{$t('tab.timeline')}</Tabs.Trigger>
        <Tabs.Trigger value="documents">{$t('tab.documents')}</Tabs.Trigger>
        <Tabs.Trigger value="reminders">
          {$t('tab.reminders')}
          {#if object.stats.due_reminder_count > 0}
            <!-- The number is for the eye; the words after it are for everyone else. -->
            <span data-testid="tab-due-badge" aria-hidden="true"
                  class="inline-grid h-5 min-w-5 place-items-center rounded-full bg-destructive px-1.5 text-xs font-semibold text-destructive-foreground tabular-nums">{object.stats.due_reminder_count}</span>
            <span class="sr-only">{$t('tab.reminders-due', { n: object.stats.due_reminder_count })}</span>
          {/if}
        </Tabs.Trigger>
        <Tabs.Trigger value="info">{$t('tab.info')}</Tabs.Trigger>
      </Tabs.List>

      <Tabs.Content value="timeline">
        {#if tab === 'timeline'}
          <Timeline
            objectId={oid} type={object.type} weightUnit={object.weight_unit} {activities} total={activityTotal} {loadingMore}
            onmore={loadMore} onlog={() => go(`/objects/${oid}/activities/new`)}
            ontriplog={offersTrip ? () => go(`/objects/${oid}/activities/new?category=trip`) : undefined}
            onchargelog={offersEnergy ? () => go(`/objects/${oid}/activities/new?category=${resourceCategory}`) : undefined}
            unit={object.counter_unit} fuelUnit={object.resource_unit ?? object.fuel_unit} resourceKind={object.resource_kind} energyRate={energyData?.cost_per_counter_milli ?? null}
            bind:category bind:tagFilter bind:titleFilter
          />
          <!-- The empty timeline puts this same action in the middle of the page, where the eye
               already is; two of them would be two calls to the same action. -->
          {#if activities.length > 0 || category !== '' || tagFilter !== null || titleFilter !== null}
            <div class="fab-row">
              <LogAction options={logChoices} onpick={(o) => go(o.path)} />
            </div>
          {/if}
        {/if}
      </Tabs.Content>
      <Tabs.Content value="documents">
        {#if tab === 'documents'}<Documents objectId={oid} coverAttachmentId={object.cover_attachment_id} onchanged={loadObject} />{/if}
      </Tabs.Content>
      <Tabs.Content value="reminders">
        {#if tab === 'reminders'}<Reminders body={object.type === 'body'} objectId={oid} unit={object.counter_unit} {activities} onchanged={() => { loadObject(); loadActivities('refresh'); }} />{/if}
      </Tabs.Content>
      <Tabs.Content value="info">
        {#if tab === 'info'}
          INFO_MARKUP
        {/if}
      </Tabs.Content>
    </Tabs.Root>
```

   `INFO_MARKUP` stands for the current Info branch body, moved verbatim with no edits: the lines
   from `<h2>{object.name}</h2>` through the `</div>` that closes `<div class="list info-actions">`.
   Those are lines 438–463 of `ObjectDetail.svelte` at `main` f8395fc. Task 3 and Task 4 rewrite
   it; this task only moves it.
3. Delete the scoped rule `.tabs .chip { margin-left: var(--space-1); }`.

- [ ] **Step 7: Delete app.css's tab rules.** Remove these three lines from
  `frontend/src/app.css`; nothing else uses `.tabs` (`grep -rn 'class="tabs' src` prints nothing):

```css
.tabs { display: flex; border-bottom: 1px solid var(--border); margin-bottom: var(--space-3); }
.tabs button:where(:not([data-slot])) { flex: 1; background: none; border-radius: 0; border-bottom: 2px solid transparent; padding: var(--space-3) var(--space-1); }
.tabs button.active:where(:not([data-slot])) { border-bottom-color: var(--accent-ink); color: var(--accent-ink); font-weight: 600; }
```

- [ ] **Step 8: `openInfo` helper.** Append to `frontend/tests-e2e/helpers.ts`:

```ts
/**
 * Opens the object page's Info tab where there is one. From 1024 px the summary pane on the left
 * holds the same content and there is no Info tab, so there is nothing to open. Waits for the
 * tab list first, so "no Info tab" is an answer about the page, not about a page still loading.
 */
export async function openInfo(page: Page): Promise<void> {
  const tabs = page.getByRole('tablist');
  await expect(tabs).toBeVisible();
  const info = tabs.getByRole('tab', { name: 'Info', exact: true });
  if ((await info.count()) > 0) await info.click();
}
```

- [ ] **Step 9: Move the specs from button to tab.** These are locator changes only, and each
  assertion keeps its meaning:
  - `02-lifecycle.spec.ts:41`: `getByRole('button', { name: 'Documents' })` becomes
    `getByRole('tab', { name: 'Documents' })`.
  - `02-lifecycle.spec.ts:45`, `15-reading-reminders.spec.ts:17,40,81` and
    `18-templates.spec.ts:27`: `getByRole('button', { name: /^Reminders/ })` becomes
    `getByRole('tab', { name: /^Reminders/ })`.
  - `11-controls.spec.ts:197,202`: `'button'` becomes `'tab'` for Documents and Reminders.
  - `12-object-hierarchy.spec.ts:28`, `23-tags.spec.ts:126`, `24-own-types.spec.ts:125,184` and
    `27-last-done.spec.ts:42,80,87`: `await page.getByRole('button', { name: 'Info'… }).click();`
    becomes `await openInfo(page);`. Import `openInfo` from `./helpers`.
  - `21-object-cost-depth.spec.ts`:
    - The local `openInfo(page, id)` body becomes
      `await page.goto(`/objects/${id}`); await openInfoTab(page);`, with
      `import { openInfo as openInfoTab, signInFresh } from './helpers';`.
    - Line 48 becomes `await openInfoTab(page);`.
  - `27-last-done.spec.ts:55` becomes
    `await expect(page.getByRole('tab', { name: 'Timeline', exact: true })).toHaveAttribute('aria-selected', 'true');`.
    The `.active` class was the old tab's look; `aria-selected` is what a tab means.
  - `27-last-done.spec.ts:91` becomes
    `await page.getByRole('tab', { name: 'Timeline', exact: true }).click();`.

- [ ] **Step 10: Verify**

Run: `npm run check && npm test && npx playwright test 37-object-detail 02-lifecycle 11-controls 12-object 15-reading 18-templates 21-object 23-tags 24-own 27-last`
Expected: PASS, both projects. Then the full `npm run e2e`: PASS.
Capture `node scripts/shots.mjs ../shots/r3-t1 object` (server as in Global Constraints) and
compare it with `shots/r3-before`. Check that the four tabs fit on one row on mobile, that the badge sits
beside "Reminders", and that the active tab is underlined in amber ink.

- [ ] **Step 11: Commit**

```bash
git add -A frontend
git commit -m "feat: object page tabs are real tabs on one row, due count inline"
```

(plus the `Co-Authored-By:` line.)

### Task 2: A `Chart` that hides empty months

**Files:**
- Create: `frontend/src/lib/chart.ts`, `frontend/src/lib/Chart.svelte`, `frontend/tests/chart.test.ts`
- Modify: `frontend/src/lib/insights.ts`, `frontend/tests/insights.test.ts`,
  `frontend/src/lib/Insights.svelte`, `frontend/tests/theme-contrast.test.ts`,
  `frontend/tests-e2e/21-object-cost-depth.spec.ts`, `frontend/tests-e2e/28-trips.spec.ts`
- Test: `frontend/tests-e2e/37-object-detail.spec.ts`

**Interfaces:**
- Produces:
  - `interface ChartBar { key: string; label: string; tick: string; value: number; display: string }`,
    plus `visibleBars`, `barShare` and `tickShown` in `chart.ts`.
  - `<Chart items={ChartBar[]} label={string} />`: bars carry `data-testid="chart-bar"`. A
    screen-reader table sits under the drawing, with `label` as its caption (and so its
    accessible name).
  - `monthTick(month, locale)` and `fillTick(date, locale)` in `insights.ts`.
  - The Insights sections carry the test ids `insights-spend` and `insights-usage`, plus the
    existing `insights-trip-distance` and `insights-by-fill`.

- [ ] **Step 1: Failing unit tests.** Create `frontend/tests/chart.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { barShare, tickShown, visibleBars, type ChartBar } from '../src/lib/chart';

const bar = (key: string, value: number): ChartBar => ({ key, label: key, tick: key, value, display: String(value) });

describe('visibleBars', () => {
  it('drops empty and unmeasured months, keeping the order', () => {
    const bars = [bar('2026-01', 0), bar('2026-02', 1200), bar('2026-03', Number.NaN), bar('2026-04', 300)];
    expect(visibleBars(bars).map((b) => b.key)).toEqual(['2026-02', '2026-04']);
  });
  it('is empty when every month is', () => {
    expect(visibleBars([bar('a', 0), bar('b', 0)])).toEqual([]);
  });
});

describe('barShare', () => {
  it('is the value against the tallest bar, within 0..1', () => {
    expect(barShare(50, 200)).toBe(0.25);
    expect(barShare(200, 200)).toBe(1);
    expect(barShare(300, 200)).toBe(1);
    expect(barShare(5, 0)).toBe(0);
  });
});

describe('tickShown', () => {
  it('labels every bar up to six', () => {
    expect([0, 1, 2, 3, 4, 5].map((i) => tickShown(i, 6))).toEqual([true, true, true, true, true, true]);
  });
  it('labels every other bar past six, always the newest', () => {
    expect([9, 10, 11].map((i) => tickShown(i, 12))).toEqual([true, false, true]);
    expect(tickShown(0, 12)).toBe(false);
    expect(tickShown(0, 7)).toBe(true);
  });
});
```

Append to `frontend/tests/insights.test.ts` (and extend its import with `fillTick, monthTick`):

```ts
describe('axis ticks', () => {
  it('a month is its short name alone', () => {
    expect(monthTick('2026-09', 'en')).toBe('Sep');
    expect(monthTick('2026-05', 'de')).toBe('Mai');
  });
  it('a fill is its day and month in numbers', () => {
    expect(fillTick('2026-01-10', 'en-US')).toBe('1/10');
    expect(fillTick('2026-01-10', 'de')).toBe('10.1.');
  });
});
```

Run: `npx vitest run tests/chart.test.ts tests/insights.test.ts`. Expected: FAIL (modules and
functions missing).

- [ ] **Step 2: `frontend/src/lib/chart.ts`**

```ts
/** One bar of a `Chart`. `value` sizes it; `display` is what a reader is told; `label` names it in
 *  full ("Sep 26") and `tick` is the short text under it ("Sep"). */
export interface ChartBar { key: string; label: string; tick: string; value: number; display: string }

/** The bars worth drawing. A month with nothing in it -- no spend, no trip, or a usage the
 *  readings cannot measure (passed in as 0) -- is left out: twelve months with two fills are two
 *  bars, not ten empty tracks. Each bar keeps its own label, so a gap shows in the labels. */
export function visibleBars(items: ChartBar[]): ChartBar[] {
  return items.filter((b) => Number.isFinite(b.value) && b.value > 0);
}

/** A bar's height as a share of the chart, 0..1, against the tallest bar. */
export function barShare(value: number, max: number): number {
  return max > 0 ? Math.min(1, Math.max(0, value / max)) : 0;
}

/** Whether bar `i` of `n` gets its tick drawn. Six fit a 300 px chart; past that every other one
 *  is drawn, counted from the newest (last) bar so the current month is always labelled. The
 *  screen-reader table under the chart names every bar either way. */
export function tickShown(i: number, n: number): boolean {
  return n <= 6 || (n - 1 - i) % 2 === 0;
}
```

- [ ] **Step 3: Ticks in `frontend/src/lib/insights.ts`.** Append:

```ts
/** `2026-09` as "Sep": a chart's axis has room for the month alone. The bar's full label (with
 *  the year, `monthLabel`) is in its tooltip and in the table screen readers read. */
export function monthTick(month: string, locale: string): string {
  const [y, m] = month.split('-').map(Number);
  return dateTimeFormat(locale, { month: 'short', timeZone: 'UTC' }).format(utc(y, m, 15));
}

/** `2026-01-10` as "1/10" (en-US) or "10.1." (de): short enough for twelve fills under a chart. */
export function fillTick(date: string, locale: string): string {
  const [y, m, d] = date.split('-').map(Number);
  return dateTimeFormat(locale, { day: 'numeric', month: 'numeric', timeZone: 'UTC' }).format(utc(y, m, d));
}
```

Run the unit tests. Expected: PASS. If `fillTick(…, 'de')` yields a different ICU spelling, assert
what `Intl.DateTimeFormat('de', { day: 'numeric', month: 'numeric', timeZone: 'UTC' })` produces
in Node, and note it in the report.

- [ ] **Step 4: `frontend/src/lib/Chart.svelte`**

```svelte
<script lang="ts">
  import { barShare, tickShown, visibleBars, type ChartBar } from './chart';

  /** A small column chart for a series over time, drawn by hand: the app has no chart library
   *  and needs only this. `label` names the series for screen readers, which read the table
   *  under the drawing rather than the drawing. Nothing renders when every bar is empty -- the
   *  caller decides whether its heading stays. */
  let { items, label }: { items: ChartBar[]; label: string } = $props();

  const bars = $derived(visibleBars(items));
  const max = $derived(Math.max(0, ...bars.map((b) => b.value)));
  const top = $derived(bars.find((b) => b.value === max));
  // viewBox units. The SVG is stretched to its box (preserveAspectRatio="none"), which is safe
  // because it holds only rectangles and lines -- every piece of text is HTML around it.
  const W = 100;
  const H = 50;
  const slot = $derived(W / Math.max(bars.length, 1));
</script>

{#if bars.length > 0}
  <figure class="m-0 flex flex-col gap-1">
    <!-- The scale: the tallest bar's value, at the dashed line it reaches. -->
    <p class="m-0 text-xs text-muted-foreground tabular-nums" aria-hidden="true">{top?.display}</p>
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" class="block h-28 w-full" aria-hidden="true" focusable="false">
      <line x1="0" y1="0.5" x2={W} y2="0.5" class="stroke-border" stroke-width="1" stroke-dasharray="2 2" vector-effect="non-scaling-stroke" />
      {#each bars as b, i (b.key)}
        {@const h = barShare(b.value, max) * (H - 1)}
        <!-- brand-ink, not the amber fill: a bar is a graphic that carries meaning, so it needs
             3:1 against the card (1.4.11); the fill is 2.1:1. -->
        <rect data-testid="chart-bar" x={i * slot + slot * 0.18} width={slot * 0.64} y={H - h} height={h} class="fill-brand-ink">
          <title>{b.label}: {b.display}</title>
        </rect>
      {/each}
      <line x1="0" y1={H - 0.5} x2={W} y2={H - 0.5} class="stroke-input" stroke-width="1" vector-effect="non-scaling-stroke" />
    </svg>
    <div class="grid text-center text-xs text-muted-foreground" style={`grid-template-columns: repeat(${bars.length}, minmax(0, 1fr))`} aria-hidden="true">
      {#each bars as b, i (b.key)}<span class="min-w-0 whitespace-nowrap">{tickShown(i, bars.length) ? b.tick : ''}</span>{/each}
    </div>
    <table class="sr-only">
      <caption>{label}</caption>
      <tbody>
        {#each bars as b (b.key)}<tr><th scope="row">{b.label}</th><td>{b.display}</td></tr>{/each}
      </tbody>
    </table>
  </figure>
{/if}
```

- [ ] **Step 5: Use it in `frontend/src/lib/Insights.svelte`**

1. Imports: add `import Chart from './Chart.svelte';`, and change the insights import to
   `import { fillLabel, fillTick, insightsPath, monthLabel, monthTick, sinceLabel } from './insights';`.
2. Replace

```svelte
    <h3>{$t('insights.spend-by-month')}</h3>
    <BarList items={data.by_month.map((b) => ({ key: b.bucket, label: monthLabel(b.bucket, $locale), value: b.cost_cents, display: fmt(b.cost_cents) }))} />
```

with

```svelte
    {#if data.by_month.some((b) => b.cost_cents > 0)}
      <section data-testid="insights-spend">
        <h3>{$t('insights.spend-by-month')}</h3>
        <Chart label={$t('insights.spend-by-month')}
               items={data.by_month.map((b) => ({ key: b.bucket, label: monthLabel(b.bucket, $locale), tick: monthTick(b.bucket, $locale), value: b.cost_cents, display: fmt(b.cost_cents) }))} />
      </section>
    {/if}
```

3. Replace the whole `{#if data.usage_by_month.length > 0 && unit}…{/if}` block with

```svelte
  {#if data.usage_by_month.some((m) => (m.amount ?? 0) > 0) && unit}
    <!-- A month the readings cannot measure is left out with the empty ones, rather than drawn
         as a zero it does not know. -->
    <section data-testid="insights-usage">
      <h3>{$t('insights.usage-by-month')}</h3>
      <Chart label={$t('insights.usage-by-month')} items={data.usage_by_month.map((m) => ({
        key: m.month, label: monthLabel(m.month, $locale), tick: monthTick(m.month, $locale), value: m.amount ?? 0,
        display: m.amount === null ? '—' : counter(m.amount, unit, $locale),
      }))} />
    </section>
  {/if}
```

4. In the `insights-trip-distance` section, replace its `<BarList … />` with

```svelte
      <Chart label={$t('trips.by-month')} items={data.trip_distance_by_month.map((m) => ({
        key: m.month, label: monthLabel(m.month, $locale), tick: monthTick(m.month, $locale), value: m.distance, display: counter(m.distance, unit, $locale),
      }))} />
```

5. In the `insights-by-fill` section, replace its `<BarList … />` with

```svelte
      <Chart label={$t('insights.by-fill')} items={fuel.fills.map((f, i) => ({
        key: `${f.date}-${i}`, label: fillLabel(f.date, $locale), tick: fillTick(f.date, $locale), value: f.per_100_milli,
        display: `${quantity(f.per_100_milli, fuelUnitLabel(fuel.unit), $locale)}/100 ${unit}`,
      }))} />
```

`BarList` stays imported for "Per year" and "Per category".

- [ ] **Step 6: Contrast.** In `theme-contrast.test.ts`, inside the `'shadcn token contrast'` loop,
  add `['brand-ink', 'card']` to the `>= 3:1` list with the comment
  `// Chart bars (Chart.svelte) are brand-ink on a card: a meaningful graphic, 1.4.11.`
  (it already passes at 4.5:1; the entry records why it matters).

- [ ] **Step 7: Failing e2e test.** Append to `37-object-detail.spec.ts`. Add
  `openInfo` to the helpers import, and add these helpers under `object()`:

```ts
async function entry(page: Page, id: number, data: Record<string, unknown>): Promise<void> {
  const res = await page.request.post(`/api/objects/${id}/activities`, { data: { notes: '', title: 'Entry', category: 'other', ...data } });
  expect(res.ok()).toBe(true);
}
/** A date `n` days back, as the API takes it. */
const daysAgo = (n: number) => new Date(Date.now() - n * 86_400_000).toISOString().slice(0, 10);
```

```ts
test('a chart leaves out the months with nothing in them', async ({ page }) => {
  await signInFresh(page, '37-chart');
  const car = await object(page, { name: 'Chart car', type: 'car', counter_unit: 'km' });
  // Two spends, 67 days apart: two different months inside the last twelve, ten empty ones.
  await entry(page, car, { date: daysAgo(3), category: 'repair', title: 'Recent', cost_cents: 10_000 });
  await entry(page, car, { date: daysAgo(70), category: 'repair', title: 'Earlier', cost_cents: 5_000 });
  await page.goto(`/objects/${car}`);
  await openInfo(page);

  await expect(page.getByTestId('insights-spend').getByTestId('chart-bar')).toHaveCount(2);
  // What a screen reader gets: one row per drawn month, named by the chart's own heading.
  await expect(page.getByRole('table', { name: 'Spend per month' }).getByRole('row')).toHaveCount(2);
});
```

Run: `npm run build && npx playwright test 37-object-detail -g "chart"`. Expected: PASS now that
Steps 2–5 are in. If you are strictly TDD-ordering, write this test before Step 5 and see it FAIL
on `insights-spend`.

- [ ] **Step 8: Move the two specs off `.bar-row`**
  - `21-object-cost-depth.spec.ts:28`: `getByTestId('insights-by-fill').locator('.bar-row')` becomes
    `getByTestId('insights-by-fill').getByTestId('chart-bar')`. The count stays 2, since every
    fill has a value.
  - `28-trips.spec.ts:109`: `getByTestId('insights-trip-distance').locator('.bar-row')).toHaveCount(12)`
    becomes `getByTestId('insights-trip-distance').getByTestId('chart-bar')).toHaveCount(1)`.
    This is a **changed assertion**: the chart no longer draws the eleven empty months. Both
    trips fall in the current month, so one bar remains. Name it in the report.

- [ ] **Step 9: Verify, look, commit**

Run: `npm run check && npm test && npm run e2e`: all PASS. Capture `../shots/r3-t2` (`object`)
and check `07-object-info`. The charts are columns, the empty months are gone, the labels do not
overlap and the bars are visible in dark mode.

```bash
git add -A frontend
git commit -m "feat: hand-drawn Chart for monthly series; empty months left out"
```

### Task 3: The summary: cover, type · since · tags, figures, due reminders

**Files:**
- Create: `frontend/src/lib/ObjectSummary.svelte`, `frontend/src/lib/lateness.ts`,
  `frontend/tests/lateness.test.ts`
- Modify: `frontend/src/app.tw.css`, `frontend/src/lib/object-detail.ts`,
  `frontend/tests/object-detail.test.ts`, `frontend/src/lib/energy.ts`,
  `frontend/tests/energy.test.ts`, `frontend/src/lib/EnergyFigures.svelte`,
  `frontend/src/lib/Insights.svelte`, `frontend/src/lib/DashboardReminders.svelte`,
  `frontend/src/routes/ObjectDetail.svelte`, `frontend/src/app.css`, `frontend/src/i18n/en.ts`,
  `frontend/src/i18n/de.ts`, `frontend/tests-e2e/24-own-types.spec.ts`,
  `frontend/tests-e2e/28-trips.spec.ts`
- Test: `frontend/tests-e2e/37-object-detail.spec.ts`

**Interfaces:**
- Produces:
  - `type FigureKey = 'cost' | 'weight' | 'counter' | 'usage' | 'consumption' | 'activities'` and
    `figureKeys(o: MemObject, hasConsumption: boolean): FigureKey[]` in `object-detail.ts`.
  - `lateness(r: Pick<Reminder, 'days_until'>, t): string` in `lateness.ts`.
  - `energyApplies(unit: string | null): boolean` in `energy.ts`.
  - `<ObjectSummary object insights due onreminders />`.
  - `Insights.svelte` becomes presentational:
    `<Insights data error unit hasContents contents oncontents />`.
  - `ObjectDetail` state: `insights`, `insightsError`, `dueReminders`, `includeContents` (store),
    `hasContents`.
  - Tailwind variants `wide:` / `max-wide:` (1024 px).

- [ ] **Step 1: Failing unit tests.** Append to `frontend/tests/object-detail.test.ts`. Add
  `figureKeys` to its import and `import type { MemObject } from '../src/lib/types';`:

```ts
const mo = (o: { type?: string; counter_unit?: 'km' | 'h' | null; stats?: Partial<MemObject['stats']> }): MemObject => ({
  type: 'car', counter_unit: null, ...o,
  stats: { total_cost_cents: 0, activity_count: 0, current_counter: null, due_reminder_count: 0, last_reading_date: null, last_activity_date: null, counter_per_day_milli: null, ...o.stats },
}) as unknown as MemObject;

describe('figureKeys', () => {
  it('a car with history: cost, counter, usage per month, consumption', () => {
    expect(figureKeys(mo({ counter_unit: 'km', stats: { counter_per_day_milli: 40_000 } }), true)).toEqual(['cost', 'counter', 'usage', 'consumption']);
  });
  it('a new car fills its row with the activity count', () => {
    expect(figureKeys(mo({ counter_unit: 'km' }), false)).toEqual(['cost', 'counter', 'activities']);
  });
  it('a drill has no counter figures', () => {
    expect(figureKeys(mo({ type: 'tool' }), false)).toEqual(['cost', 'activities']);
  });
  it('consumption needs a counter to be "per 100" of', () => {
    expect(figureKeys(mo({ type: 'home' }), true)).toEqual(['cost', 'activities']);
  });
  it('a body shows its weight instead of a cost', () => {
    expect(figureKeys(mo({ type: 'body', stats: { latest_weight_grams: 80_500 } }), false)).toEqual(['weight', 'activities']);
    expect(figureKeys(mo({ type: 'body' }), false)).toEqual(['activities']);
  });
});
```

Create `frontend/tests/lateness.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import { lateness } from '../src/lib/lateness';

const t = (k: string, v?: Record<string, string | number>) => (v ? `${k}:${JSON.stringify(v)}` : k);

describe('lateness', () => {
  it('says nothing about a reminder with no date', () => {
    expect(lateness({ days_until: null }, t)).toBe('');
  });
  it('today, one day, many days', () => {
    expect(lateness({ days_until: 0 }, t)).toBe('dash.due-today');
    expect(lateness({ days_until: -1 }, t)).toBe('dash.overdue-day');
    expect(lateness({ days_until: -12 }, t)).toBe('dash.overdue-days:{"n":12}');
  });
  it('says nothing about one not due yet', () => {
    expect(lateness({ days_until: 3 }, t)).toBe('');
  });
});
```

Append to `frontend/tests/energy.test.ts` (and import `energyApplies`):

```ts
describe('energyApplies', () => {
  it('is for objects that charge', () => {
    expect(energyApplies('kwh')).toBe(true);
    expect(energyApplies('l')).toBe(false);
    expect(energyApplies('gal')).toBe(false);
    expect(energyApplies(null)).toBe(false);
  });
});
```

Run: `npx vitest run tests/object-detail.test.ts tests/lateness.test.ts tests/energy.test.ts`. Expected: FAIL.

- [ ] **Step 2: Implement the three helpers**

Append to `frontend/src/lib/object-detail.ts`:

```ts
/** A figure the object page's summary can show. */
export type FigureKey = 'cost' | 'weight' | 'counter' | 'usage' | 'consumption' | 'activities';

/**
 * Which figures the summary shows for `o`, most telling first, at most four: what it cost (a
 * body's latest weight instead -- a person has no running cost), its counter, how far it goes a
 * month, and its consumption when there is one to show. A figure that does not apply to the
 * object is left out rather than shown as a dash; the activity count fills a short row, so a
 * drill's summary is not one lonely card.
 */
export function figureKeys(o: MemObject, hasConsumption: boolean): FigureKey[] {
  const keys: FigureKey[] = [];
  if (o.type !== 'body') keys.push('cost');
  else if (o.stats.latest_weight_grams != null) keys.push('weight');
  if (o.counter_unit) {
    keys.push('counter');
    if (o.stats.counter_per_day_milli !== null) keys.push('usage');
    if (hasConsumption) keys.push('consumption');
  }
  if (keys.length < 4) keys.push('activities');
  return keys;
}
```

Create `frontend/src/lib/lateness.ts`:

```ts
import type { Reminder } from './types';

type Translate = (key: string, vars?: Record<string, string | number>) => string;

/** How late a due reminder is, when it is due by date: "due today", "1 day overdue", "12 days
 *  overdue". A counter-only reminder has no date to be late against, so it says nothing; its
 *  card already says it is due. Shared by the dashboard's reminder cards and the object page's
 *  summary. */
export function lateness(r: Pick<Reminder, 'days_until'>, t: Translate): string {
  if (r.days_until === null) return '';
  if (r.days_until === 0) return t('dash.due-today');
  if (r.days_until === -1) return t('dash.overdue-day');
  if (r.days_until < 0) return t('dash.overdue-days', { n: -r.days_until });
  return '';
}
```

Append to `frontend/src/lib/energy.ts`:

```ts
/**
 * Whether the Energy section's figures mean anything for an object with this fuel unit. They are
 * charging figures -- distance per charge, per kWh, when to charge next -- and on a diesel car
 * they read as nonsense (the audit found "Distance per charge" there). A liquid fuel's
 * consumption and cost per distance are already in the Cost section. The energy rate itself is
 * still used for every fuel (trip cost estimates); only the section is gated.
 */
export function energyApplies(unit: string | null): boolean {
  return unit === 'kwh';
}
```

In `DashboardReminders.svelte`, replace its local `lateness` function (the whole function
and its doc comment) with:

```ts
  import { lateness as latenessOf } from './lateness';
  /** See ./lateness.ts. */
  const lateness = (r: Reminder) => latenessOf(r, $t);
```

(Move the `import` line up to the other imports.) Its template is unchanged.

In `EnergyFigures.svelte`, change the import to `import { energyApplies, formatPerUnit, fuelUnitLabel } from './energy';`
and the start of `hasAny` to `energy !== null && energyApplies(energy.unit)`, followed by the
existing `&& (…)`. Add to its doc comment: "Charging objects only -- see `energyApplies`."

Run the unit tests again. Expected: PASS.

- [ ] **Step 3: Breakpoint.** In `frontend/src/app.tw.css`'s `@theme { … }` block, after
  `--breakpoint-desk: 900px;`, add:

```css
  /* The object page's two panes (routes/ObjectDetail.svelte). In px, not Tailwind's rem `lg`,
     because the page's one script-side query must read the same width: `(width >= 1024px)`. */
  --breakpoint-wide: 1024px;
```

- [ ] **Step 4: i18n.** In `en.ts`, after `'object.contents-add': 'New object inside',`:

```ts
  'object.summary': 'Summary',
  'object.since-short': 'since {date}',
  'object.per-month': 'Per month',
  'object.figures': 'Figures',
```

In `de.ts`, at the same place:

```ts
  'object.summary': 'Übersicht',
  'object.since-short': 'seit {date}',
  'object.per-month': 'Pro Monat',
  'object.figures': 'Kennzahlen',
```

- [ ] **Step 5: `frontend/src/lib/ObjectSummary.svelte`**

```svelte
<script lang="ts">
  import { fileUrl } from './api';
  import TagChips from './TagChips.svelte';
  import { counter, money, quantity } from './format';
  import { fuelUnitLabel } from './energy';
  import { sinceLabel } from './insights';
  import { lateness } from './lateness';
  import { figureKeys, type FigureKey } from './object-detail';
  import { formatWeight } from './weight';
  import { customTypes, typeLabel, typesLoaded } from './type-registry';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { Insights, MemObject, Reminder } from './types';

  /** The top of the object page: what it is and where it stands. One piece of markup for both
   *  layouts -- above the tabs on a phone, the head of the left pane from 1024 px; CSS moves it
   *  (see routes/ObjectDetail.svelte). `insights` is `null` until loaded; only the consumption
   *  figure waits for it. `due` is the object's due reminders; tapping one opens the Reminders
   *  tab, where the actions are. */
  let { object, insights, due, onreminders }: {
    object: MemObject; insights: Insights | null; due: Reminder[]; onreminders: () => void;
  } = $props();

  const LABEL: Record<FigureKey, string> = {
    cost: 'object.total', weight: 'weight.latest', counter: 'object.current', usage: 'object.per-month',
    consumption: 'insights.consumption', activities: 'object.activities',
  };
  const keys = $derived(figureKeys(object, insights?.fuel?.per_100_milli != null));

  function value(key: FigureKey): string {
    const s = object.stats;
    switch (key) {
      case 'cost': return money(s.total_cost_cents, $currency, $locale);
      case 'weight': return formatWeight(s.latest_weight_grams ?? 0, object.weight_unit ?? 'kg', $locale);
      case 'counter': return counter(s.current_counter, object.counter_unit, $locale) || '—';
      // A month is the unit people think in; rounded to whole units, since the rate is an average.
      case 'usage': return counter(Math.round((s.counter_per_day_milli ?? 0) * 30.44 / 1000), object.counter_unit, $locale);
      case 'consumption': {
        const f = insights?.fuel;
        return f?.per_100_milli != null ? `${quantity(f.per_100_milli, fuelUnitLabel(f.unit), $locale)}/100 ${object.counter_unit}` : '—';
      }
      case 'activities': return String(s.activity_count);
    }
  }

  /** Stretched link, as on the dashboard: the title's ::after covers the card, so all of it is the
   *  target, and the ring is drawn on that ::after. */
  const stretched = "after:absolute after:inset-0 after:rounded-lg after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-solid focus-visible:after:outline-offset-2 focus-visible:after:outline-ring";
</script>

{#if object.cover_file_id}
  <!-- A fixed 16:10 box, cropped to fit: the old free-height hero cut faces and number plates at
       whatever height the photo happened to have. The thumbnail first (usually cached from the
       dashboard card), the original over it once loaded; keyed so a new cover starts from its own
       thumbnail. -->
  {#key object.cover_file_id}
    <div data-testid="object-cover" class="relative aspect-[16/10] w-full overflow-hidden rounded-lg bg-muted">
      <img class="absolute inset-0 block size-full object-cover" src={fileUrl(object.cover_file_id, true)} alt="" decoding="async" />
      <img class="absolute inset-0 block size-full object-cover opacity-0 transition-opacity duration-150" src={fileUrl(object.cover_file_id)} alt="" decoding="async"
           onload={(e) => e.currentTarget.classList.replace('opacity-0', 'opacity-100')} />
    </div>
  {/key}
{/if}

<div class="flex flex-col gap-1.5">
  <p class="m-0 text-sm text-muted-foreground">
    <span data-testid="object-type">{typeLabel(object.type, $customTypes, $t, $typesLoaded)}</span>{#if object.purchase_date}{' · '}{$t('object.since-short', { date: sinceLabel(object.purchase_date, $locale) })}{/if}
  </p>
  {#if (object.tags ?? []).length > 0}<div class="w-fit"><TagChips tags={object.tags} /></div>{/if}
</div>

{#if keys.length > 0}
  <!-- A strip that scrolls sideways on a phone (the fade says there is more), a 2×2 grid in the
       pane. Focusable, so a keyboard can scroll it; the fade is dropped while it has focus so it
       does not dim the ring. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div role="group" aria-label={$t('object.figures')} tabindex="0"
       class="-m-1 overflow-x-auto p-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden max-wide:[mask-image:linear-gradient(to_right,#000_calc(100%_-_24px),transparent)] max-wide:focus-visible:[mask-image:none] focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring wide:overflow-visible">
    <dl data-testid="figures" class="m-0 flex gap-2 wide:grid wide:grid-cols-2">
      {#each keys as key (key)}
        <div data-testid={`figure-${key}`} class="flex min-w-32 shrink-0 flex-col rounded-lg border border-border bg-card p-3 shadow-xs wide:min-w-0">
          <!-- Label first for a screen reader, number first for the eye. -->
          <dt class="order-2 text-xs text-muted-foreground">{$t(LABEL[key])}</dt>
          <dd class="order-1 m-0 truncate text-lg font-semibold text-foreground tabular-nums">{value(key)}</dd>
        </div>
      {/each}
    </dl>
  </div>
{/if}

{#if due.length > 0}
  <div class="flex flex-col gap-2">
    {#each due as r (r.id)}
      <div data-testid="summary-due-reminder" class="relative flex items-center gap-3 rounded-lg border border-destructive/30 bg-destructive/10 p-3">
        <span class="size-2 shrink-0 rounded-full bg-destructive" aria-hidden="true"></span>
        <div class="min-w-0 flex-1">
          <a href={`/objects/${object.id}?tab=reminders`} class={`block break-words font-semibold text-foreground no-underline ${stretched}`}
             onclick={(e) => { e.preventDefault(); onreminders(); }}>{r.title}</a>
          {#if lateness(r, $t)}<p class="m-0 text-sm text-muted-foreground">{lateness(r, $t)}</p>{/if}
        </div>
      </div>
    {/each}
  </div>
{/if}
```

- [ ] **Step 6: `Insights.svelte` becomes presentational.** Replace its `<script>` block and its
  `{#if hasContents}…{/if}` toggle with the code below. The rest of the template is unchanged,
  except that `{#if error}…{/if}` now reads the `error` prop:

```svelte
<script lang="ts">
  import BarList from './BarList.svelte';
  import Chart from './Chart.svelte';
  import { counter, money, moneyWhole, perCounter, quantity } from './format';
  import { energyLabelKey, fuelUnitLabel } from './energy';
  import { fillLabel, fillTick, monthLabel, monthTick, sinceLabel } from './insights';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { CounterUnit, FuelUnit, Insights } from './types';

  /** Loaded by ObjectDetail, which needs the same answer for the summary's consumption figure
   *  and the spend chart: one request, not two. `null` until it arrives. `contents` is the
   *  "Include contents" switch; ObjectDetail owns it because it decides which request is made.
   *  `hasContents` offers the switch only where there is something to include. */
  let { data, error = '', unit, hasContents = false, contents = false, oncontents }: {
    data: Insights | null; error?: string; unit: CounterUnit; hasContents?: boolean; contents?: boolean;
    oncontents?: (on: boolean) => void;
  } = $props();

  const fmt = (cents: number) => money(cents, $currency, $locale);
  const spent = $derived(data ? data.by_year.some((b) => b.cost_cents > 0) : false);
</script>

{#if hasContents}
  <label class="row toggle">
    <input type="checkbox" checked={contents} onchange={(e) => oncontents?.(e.currentTarget.checked)} />
    {$t('insights.contents')}
  </label>
{/if}
```

- [ ] **Step 7: `ObjectDetail.svelte` loads insights and due reminders, and shows the summary**

1. Imports: add `import ObjectSummary from '../lib/ObjectSummary.svelte';`,
   `import { persisted } from '../stores/persisted';` and
   `import { insightsPath } from '../lib/insights';`. Extend the types import with
   `Insights as InsightsData, Reminder`.
2. After `let energyData = …;`, add:

```ts
  /** The Cost data: the summary's consumption figure, the Info tab's breakdown and (from
   *  1024 px) the pane's spend chart. Loaded once here rather than by each. `null` until loaded. */
  let insights = $state<InsightsData | null>(null);
  let insightsError = $state('');
  const insightsSeq = createSeq();
  /** Per device and not synced, like the Statistics screen's purchase switch: a way of looking, not data. */
  const includeContents = persisted('logb.insights.contents', false);
  /** Archived children are not listed under Contents, but their costs still count with "Include
   *  contents", so having any is enough to offer the switch. */
  const hasContents = $derived(children.length > 0 || archivedChildCount > 0);
  // An object without children always asks for its own figures, whatever the switch last said
  // on a house. Derived, so a `hasContents` flip that does not change the path does not refetch.
  const insightsUrl = $derived(insightsPath(oid, hasContents && $includeContents));

  async function loadInsights(path: string) {
    const token = insightsSeq.next();
    try {
      const d = await api<InsightsData>('GET', path);
      if (insightsSeq.current(token)) { insights = d; insightsError = ''; }
    } catch (e) {
      if (insightsSeq.current(token)) insightsError = errorMessage(e, $t);
    }
  }

  /** This object's due reminders, for the summary. Asked for only when the object says it has
   *  any, so an object with none costs no request. A failure just leaves the list empty: the
   *  Reminders tab is where they are managed, and its badge still counts them. */
  let dueReminders = $state<Reminder[]>([]);
  const dueSeq = createSeq();
  const dueCount = $derived(object?.stats.due_reminder_count ?? 0);
  async function loadDue() {
    const token = dueSeq.next();
    try {
      const rows = await api<Reminder[]>('GET', `/objects/${oid}/reminders`);
      if (dueSeq.current(token)) dueReminders = rows.filter((r) => r.due && r.done_at === null);
    } catch {
      if (dueSeq.current(token)) dueReminders = [];
    }
  }
```

3. In the oid-reset effect, after `childrenSeq.invalidate();`, add:

```ts
    insights = null;
    insightsError = '';
    insightsSeq.invalidate();
    dueReminders = [];
    dueSeq.invalidate();
```

4. After the `$effect(() => { oid; if (offersEnergy) loadEnergy(); });` line, add:

```ts
  $effect(() => { const path = insightsUrl; loadInsights(path); });
  // `dueCount` is derived, so a reload of the object that leaves the count alone does not ask again.
  $effect(() => { oid; if (dueCount > 0) loadDue(); else { dueSeq.invalidate(); dueReminders = []; } });
```

5. In the `onOutboxFlushed` handler, after `loadActivities('refresh');`, add
   `loadInsights(insightsUrl);` (a replayed entry changes the costs too).
6. The Reminders panel's `onchanged` becomes
   `() => { loadObject(); loadActivities('refresh'); if (dueCount > 0) loadDue(); }`.
7. Template: replace the whole `{#if object.cover_file_id}…{/if}` hero block **and** the
   `<div class="stats">…</div>` block with:

```svelte
    <section aria-label={$t('object.summary')} class="mb-4 flex flex-col gap-4">
      <ObjectSummary {object} {insights} due={dueReminders} onreminders={() => setTab('reminders')} />
    </section>
```

8. In the Info panel, delete `<h2>{object.name}</h2>`, the
   `<p class="muted">{typeLabel(…)}</p>` line and `<TagChips tags={object.tags ?? []} />`, because
   the summary shows them on every tab now. Replace
   `<Insights objectId={oid} unit={object.counter_unit} hasContents={children.length > 0 || archivedChildCount > 0} />` with:

```svelte
      <Insights data={insights} error={insightsError} unit={object.counter_unit} {hasContents}
                contents={$includeContents} oncontents={(on) => includeContents.set(on)} />
```

9. Delete the scoped `.hero-wrap`, `.hero`, `.hero.full` and `.hero.full:global(.loaded)` rules.
   Remove the imports that `npm run check` now reports as unused (`fileUrl`, `counter`,
   `fmtDate`, `dateFormat`, `TagChips` if unused).

- [ ] **Step 8: app.css.** Delete the `.stat { … }` line, the whole `.stats { … }` rule, and the
  `@media (max-width: 479px) { .stats:has(…) … }` block with its comment. Then change
  `.stat b, .tnum { font-variant-numeric: tabular-nums; }` to
  `.tnum { font-variant-numeric: tabular-nums; }`, keeping the comment above it. Before deleting,
  `grep -rn 'class="stat' src` prints nothing.

- [ ] **Step 9: Failing e2e tests.** Append to `37-object-detail.spec.ts` and add `pngPayload`
  to the helpers import:

```ts
test('the summary shows the figures that apply, and nothing about charging on a diesel car', async ({ page }) => {
  await signInFresh(page, '37-figures');
  const car = await object(page, { name: 'Figures diesel', type: 'car', counter_unit: 'km', fuel_unit: 'l' });
  for (const [date, counter_value, quantity_milli, cost_cents] of [
    ['2026-01-01', 10_000, 40_000, 6_000], ['2026-02-01', 10_500, 30_000, 4_500], ['2026-03-01', 11_000, 25_000, 3_800],
  ] as const) {
    await entry(page, car, { date, category: 'fuel', counter_value, quantity_milli, cost_cents, charged_full: 1 });
  }
  const drill = await object(page, { name: 'Figures drill', type: 'tool' });

  await page.goto(`/objects/${car}`);
  await expect(page.getByTestId('figure-cost')).toContainText('€143.00');
  await expect(page.getByTestId('figure-counter')).toContainText('11,000 km');
  await expect(page.getByTestId('figure-consumption')).toContainText('l/100 km');
  // Full fills make a "distance per charge" computable; it is still not a thing a diesel car has.
  await openInfo(page);
  await expect(page.getByRole('heading', { name: 'Consumption per fill' })).toBeVisible();
  await expect(page.getByText(/Distance per charge/)).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Energy', exact: true })).toHaveCount(0);

  await page.goto(`/objects/${drill}`);
  await expect(page.getByTestId('figure-cost')).toBeVisible();
  await expect(page.getByTestId('figure-activities')).toBeVisible();
  await expect(page.getByTestId('figure-counter')).toHaveCount(0);
  await expect(page.getByTestId('figure-consumption')).toHaveCount(0);
});

test('a due reminder in the summary opens the Reminders tab; the cover keeps 16:10', async ({ page }) => {
  await signInFresh(page, '37-summary');
  const car = await object(page, { name: 'Summary car', type: 'car', counter_unit: 'km' });
  expect((await page.request.post(`/api/objects/${car}/reminders`, { data: { title: 'Summary overdue', due_date: '2000-01-01' } })).ok()).toBe(true);
  await page.goto(`/objects/${car}`);

  const due = page.getByTestId('summary-due-reminder').filter({ hasText: 'Summary overdue' });
  await expect(due).toContainText(/days overdue/);
  await due.click();
  await expect(page.getByRole('tab', { name: /^Reminders/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page).toHaveURL(/\?tab=reminders$/);

  await page.getByRole('tab', { name: 'Documents', exact: true }).click();
  await page.getByRole('button', { name: /Add photos or files/ }).click();
  await page.setInputFiles('input[type=file]', pngPayload());
  await page.getByRole('button', { name: 'Use as cover' }).click();
  const cover = page.getByTestId('object-cover');
  await expect(cover).toBeVisible();
  const box = (await cover.boundingBox())!;
  expect(box.width / box.height).toBeCloseTo(1.6, 1);
});
```

Run: `npm run build && npx playwright test 37-object-detail -g "summary"`. Expected: PASS with Steps 2–8
in place; when strictly ordering, run this before Step 7 and see it FAIL on `figure-cost`.

- [ ] **Step 10: Move two specs off the removed markup**
  - `28-trips.spec.ts:56`: `page.locator('.stat', { hasText: 'Current' })` becomes
    `page.getByTestId('figure-counter')`. It keeps `toContainText('600 km')`.
  - `24-own-types.spec.ts`, both places:
    `await expect(page.locator('main .muted').first()).toHaveText('Pedelec')` (and `'Widget'`)
    becomes `await expect(page.getByTestId('object-type')).toHaveText('Pedelec')` (and `'Widget'`).
    The `openInfo(page)` line before each stays; it is harmless. Change the comments above it to
    "The type line is in the summary, on every tab". This is a **changed assertion**: the type
    now shows on every tab, where it used to be under Info only.

- [ ] **Step 11: Verify, look, commit**

Run: `npm run check && npm test && npm run e2e`: all PASS. Capture `../shots/r3-t3`:
- On a phone the figure strip scrolls, with a fade at the right edge.
- The cover (the seed has none; if needed, set one by hand once) is 16:10.
- The type · since line sits under it.
- The overdue "Winter tyres on" card in the seed shows in the summary, in both themes.

```bash
git add -A frontend
git commit -m "feat: object summary with cover, figures and due reminders; Energy only for charging"
```

### Task 4: Two panes on desktop; details in one place; "+ Log" and Edit in the header

**Files:**
- Create: `frontend/src/lib/ObjectDetails.svelte`
- Modify: `frontend/src/routes/ObjectDetail.svelte`, `frontend/src/lib/LogAction.svelte`,
  `frontend/src/lib/Insights.svelte`, `frontend/src/lib/LastDone.svelte`,
  `frontend/src/lib/TripTotals.svelte`, `frontend/src/lib/EnergyFigures.svelte`,
  `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`, `frontend/tests-e2e/27-last-done.spec.ts`
- Test: `frontend/tests-e2e/37-object-detail.spec.ts`

**Interfaces:**
- Consumes: Task 3's `insights`, `insightsError`, `hasContents`, `includeContents` and `dueReminders`.
- Produces:
  - `<ObjectDetails object insights insightsError lastDone tripSummary energy inside hasContents contents offersTrip offersEnergy oncontents onlastdone onimported />`.
  - `<LogAction … placement?: 'fab' | 'header' />`, defaulting to `'fab'`.
  - Last-done rows carry `data-testid="last-done-row"`.

- [ ] **Step 1: Failing e2e tests.** Append to `37-object-detail.spec.ts`:

```ts
test('from 1024 px the summary is a sticky pane beside the tabs, with no Info tab', async ({ page }, info) => {
  test.skip(info.project.name !== 'desktop', 'the two panes start at 1024 px');
  await signInFresh(page, '37-panes');
  const car = await object(page, { name: 'Panes car', type: 'car', counter_unit: 'km', fuel_unit: 'l' });
  for (let i = 0; i < 30; i++) {
    await entry(page, car, { date: `2026-0${1 + (i % 8)}-1${i % 10}`, category: 'maintenance', title: `Pane entry ${i}`, cost_cents: 1000 });
  }
  await page.goto(`/objects/${car}`);

  const summary = page.getByRole('region', { name: 'Summary' });
  const tablist = page.getByRole('tablist');
  await expect(tablist).toBeVisible();
  await expect(tablist.getByRole('tab', { name: 'Info' })).toHaveCount(0);
  const s = (await summary.boundingBox())!;
  const tl = (await tablist.boundingBox())!;
  expect(s.x + s.width).toBeLessThanOrEqual(tl.x);
  expect(s.width).toBeGreaterThan(290);
  expect(s.width).toBeLessThan(320);
  // What the Info tab held is in the pane.
  await expect(summary.getByRole('heading', { name: 'Contents' })).toBeVisible();
  await expect(summary.getByRole('button', { name: /New object inside/ })).toBeVisible();

  // "+ Log" (a menu for a car with fuel) and Edit are in the page header, not floating.
  const header = page.getByRole('main').locator('header').first();
  await expect(header.getByTestId('log-menu')).toBeVisible();
  await expect(header.getByRole('button', { name: 'Edit' })).toBeVisible();

  // The pane stays in view while the timeline scrolls.
  await page.evaluate(() => window.scrollTo(0, 1500));
  expect(await page.evaluate(() => window.scrollY)).toBeGreaterThan(1000);
  const y = (await summary.boundingBox())!.y;
  expect(y).toBeGreaterThanOrEqual(60);
  expect(y).toBeLessThanOrEqual(100);

  // An old ?tab=info link lands on the timeline; the details are already beside it.
  await page.goto(`/objects/${car}?tab=info`);
  await expect(page.getByRole('tab', { name: 'Timeline', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page).toHaveURL(new RegExp(`/objects/${car}$`));
});

test('below 1024 px the summary sits above the tabs and the rest is under Info', async ({ page }, info) => {
  test.skip(info.project.name !== 'mobile', 'the one-column layout');
  await signInFresh(page, '37-one-column');
  const car = await object(page, { name: 'Column car', type: 'car', counter_unit: 'km', fuel_unit: 'l' });
  await entry(page, car, { date: '2026-02-01', category: 'maintenance', title: 'Column entry' });
  await page.goto(`/objects/${car}`);

  const summary = page.getByRole('region', { name: 'Summary' });
  await expect(page.getByRole('tablist')).toBeVisible();
  expect((await summary.boundingBox())!.y).toBeLessThan((await page.getByRole('tablist').boundingBox())!.y);
  await expect(page.getByRole('heading', { name: 'Contents' })).toHaveCount(0);
  // "+ Log" floats; the header holds only Edit.
  await expect(page.getByRole('main').locator('header').first().getByTestId('log-menu')).toHaveCount(0);
  await expect(page.getByTestId('log-menu')).toBeVisible();
  await page.getByRole('tab', { name: 'Info', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Contents' })).toBeVisible();
});
```

Run: `npm run build && npx playwright test 37-object-detail -g "1024"`. Expected: FAIL. There is
no "Summary" region beside the tabs yet, and Info still exists on desktop.

- [ ] **Step 2: i18n.** `en.ts`: `'object.breadcrumb': 'Path',`. `de.ts`: `'object.breadcrumb': 'Pfad',`.
  Both go after `'object.figures'`.

- [ ] **Step 3: `LogAction.svelte` learns a header placement.** Replace its `<script>` and the
  `DropdownMenu.Content` opening tag:

```svelte
<script lang="ts">
  import { Button } from '$lib/components/ui/button/index.js';
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu/index.js';
  import { t } from '../i18n';
  import type { LogOption } from './log-options';

  let { options, onpick, placement = 'fab' }: {
    options: LogOption[]; onpick: (option: LogOption) => void;
    /** `fab`: the floating pill at the bottom of a phone screen. `header`: a regular button in the
     *  desktop page header, beside Edit. */
    placement?: 'fab' | 'header';
  } = $props();

  // A menu of one is a detour: with only activities to log, the button is the action itself.
  const cls = $derived(placement === 'fab' ? 'h-12 rounded-full px-5 text-base font-semibold shadow-lg' : 'min-h-11 px-4 text-sm font-semibold');
</script>
```

In the template, change both `class={fab}` to `class={cls}`. Replace the comment and the
`<DropdownMenu.Content side="top" …>` tag with:

```svelte
    <!-- Upward from the floating button (a menu opening down would open off the screen), downward
         from the header. -->
    <DropdownMenu.Content side={placement === 'fab' ? 'top' : 'bottom'} align="end" sideOffset={8} class="min-w-48">
```

- [ ] **Step 4: Section headings in the detail components.** The heading class, used verbatim
  below, is `m-0 mb-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase`. These
  are `h2`, because they sit directly under the page's `h1`.

`LastDone.svelte`: replace everything below `</script>` with the markup below, then delete its
`<style>` block:

```svelte
{#if items.length > 0}
  <section>
    <h2 class="m-0 mb-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase">{$t('lastdone.title')}</h2>
    <div class="flex flex-col gap-2">
      {#each items as row (row.last_activity_id)}
        {@const since = sinceCounter(object.stats.current_counter, row.last_counter)}
        <!-- `onselect(row.title)` passes the real title: the timeline's title filter matches on it. -->
        <button data-slot="last-done-row" data-testid="last-done-row" onclick={() => onselect(row.title)}
                class="flex min-h-11 w-full cursor-pointer flex-col gap-0.5 rounded-lg border border-border bg-card p-3 text-left shadow-xs transition-colors hover:border-input focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">
          <span class="font-semibold text-foreground">{activityTitle(row.title, undefined, $t)}</span>
          <span class="text-sm text-muted-foreground tabular-nums">
            {fmtDate(row.last_date, $dateFormat)}
            {#if row.last_counter !== null} · {counter(row.last_counter, object.counter_unit, $locale)}{/if}
            {#if since !== null} · {$t('lastdone.since', { amount: counter(since, object.counter_unit, $locale) })}{/if}
          </span>
        </button>
      {/each}
    </div>
  </section>
{/if}
```

`TripTotals.svelte`: replace `<h3>{$t('trips.title')}</h3>` with
`<section><h2 class="m-0 mb-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase">{$t('trips.title')}</h2>`,
and close `</section>` after the `table-wrap` div. The table's scoped styles stay; they style
the table, not what this round restyles.

`EnergyFigures.svelte`: the same for `<h3>{$t('energy.title')}</h3>`. Wrap the heading and its
`<p>` rows in `<section>…</section>`.

`Insights.svelte`:
- Delete the whole `{#if data.by_month.some(…)}<section data-testid="insights-spend">…</section>{/if}`
  block. The spend chart moves to the top of `ObjectDetails`.
- Replace every remaining `<h3>` opening tag with `<h3 class="m-0 mt-4 mb-2 text-sm font-semibold text-foreground">`.
- Replace `<p class="muted hint">` with `<p class="m-0 mb-2 text-sm text-muted-foreground">`.
- Delete the `<style>` block.
- Remove `monthTick` from its import if nothing else there uses it.

- [ ] **Step 5: `frontend/src/lib/ObjectDetails.svelte`**

```svelte
<script lang="ts">
  import { Button } from '$lib/components/ui/button/index.js';
  import { go } from './router';
  import Chart from './Chart.svelte';
  import Insights from './Insights.svelte';
  import LastDone from './LastDone.svelte';
  import TripTotals from './TripTotals.svelte';
  import EnergyFigures from './EnergyFigures.svelte';
  import ObjectCard from './ObjectCard.svelte';
  import ResourceCsvImport from './ResourceCsvImport.svelte';
  import { money } from './format';
  import { monthLabel, monthTick } from './insights';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { EnergyOut, Insights as InsightsData, LastDone as LastDoneT, MemObject, TripSummary } from './types';

  /** Everything about the object that is not its summary or its history: spend per month, its
   *  description, last done, trips, energy, contents, the cost breakdown, and the two actions
   *  that close it. Rendered in exactly one place -- the left pane from 1024 px, the Info tab
   *  below that; ObjectDetail picks, and loads the data it takes. */
  let { object, insights, insightsError, lastDone, tripSummary, energy, inside, hasContents, contents, offersTrip, offersEnergy, oncontents, onlastdone, onimported }: {
    object: MemObject; insights: InsightsData | null; insightsError: string; lastDone: LastDoneT[];
    tripSummary: TripSummary | null; energy: EnergyOut | null;
    /** The object's live children, listed under Contents. */
    inside: MemObject[];
    hasContents: boolean; contents: boolean; offersTrip: boolean; offersEnergy: boolean;
    oncontents: (on: boolean) => void; onlastdone: (title: string) => void; onimported: () => void;
  } = $props();

  const heading = 'm-0 mb-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase';
  const spend = $derived(insights && insights.by_month.some((b) => b.cost_cents > 0) ? insights.by_month : null);
</script>

<div class="flex flex-col gap-6">
  {#if spend}
    <section data-testid="insights-spend">
      <h2 class={heading}>{$t('insights.spend-by-month')}</h2>
      <Chart label={$t('insights.spend-by-month')}
             items={spend.map((b) => ({ key: b.bucket, label: monthLabel(b.bucket, $locale), tick: monthTick(b.bucket, $locale), value: b.cost_cents, display: money(b.cost_cents, $currency, $locale) }))} />
    </section>
  {/if}

  {#if object.description || object.purchase_price_cents !== null}
    <section class="flex flex-col gap-1">
      {#if object.description}<p class="m-0 whitespace-pre-wrap text-sm text-foreground">{object.description}</p>{/if}
      {#if object.purchase_price_cents !== null}
        <p class="m-0 text-sm text-muted-foreground">{$t('object.purchase-price')}: {money(object.purchase_price_cents, $currency, $locale)}</p>
      {/if}
    </section>
  {/if}

  <LastDone items={lastDone} {object} onselect={onlastdone} />
  {#if offersTrip}
    <TripTotals summary={tripSummary} unit={object.counter_unit as 'km' | 'mi'} energyRate={energy?.cost_per_counter_milli ?? null} />
  {/if}
  {#if offersEnergy}<EnergyFigures {energy} unit={object.counter_unit} />{/if}

  <section>
    <h2 class={heading}>{$t('object.contents')}</h2>
    {#if inside.length === 0}
      <p class="m-0 text-sm text-muted-foreground">{$t('object.contents-empty')}</p>
    {:else}
      <div class="grid grid-cols-1 gap-3">
        {#each inside as c (c.id)}<ObjectCard object={c} />{/each}
      </div>
    {/if}
  </section>

  <section>
    <h2 class={heading}>{$t('insights.title')}</h2>
    <Insights data={insights} error={insightsError} unit={object.counter_unit} {hasContents} {contents} {oncontents} />
  </section>

  {#if object.resource_kind}<ResourceCsvImport objectId={object.id} mode={object.measurement_mode} {onimported} />{/if}

  <div class="flex flex-col gap-2">
    <Button variant="outline" class="min-h-11 w-full" onclick={() => go(`/objects/new?parent_id=${object.id}`)}>+ {$t('object.contents-add')}</Button>
    <Button variant="outline" class="min-h-11 w-full" href={`/api/export?object_id=${object.id}`}>{$t('object.export')}</Button>
  </div>
</div>
```

(Edit is no longer repeated at the end. It is in the page header at every width.)

- [ ] **Step 6: `ObjectDetail.svelte`: the layout**

Script changes:

1. Imports: add `import { MediaQuery } from 'svelte/reactivity';`,
   `import { Button } from '$lib/components/ui/button/index.js';` and
   `import ObjectDetails from '../lib/ObjectDetails.svelte';`. Remove `Insights`, `ObjectCard`,
   `TagChips`, `LastDone`, `TripTotals`, `EnergyFigures`, `ResourceCsvImport`, `typeLabel`,
   `typesLoaded` and `money`, along with anything else `npm run check` reports unused.
2. After `function setTab(x: Tab) { tab = x; }`, add:

```ts
  /** The one layout switch read in script (the plan's Decision 2): from 1024 px the details sit
   *  in the left pane and there is no Info tab. It is structural -- which container owns the
   *  details, whether a tab exists, when their data loads -- so CSS cannot make it alone. The
   *  query text is exactly Tailwind's `wide:` variant (app.tw.css). Svelte's MediaQuery reads
   *  matchMedia when this mounts, so the first paint is right, and follows resizes. */
  const wide = new MediaQuery('(width >= 1024px)');
  /** An old `?tab=info` at desktop width shows the timeline: the details are already on screen. */
  const shownTab = $derived<Tab>(wide.current && tab === 'info' ? 'timeline' : tab);
  $effect(() => { if (wide.current && tab === 'info') tab = 'timeline'; });
  /** Whether the details are on screen: always from 1024 px, else while Info is open. */
  const detailsShown = $derived(wide.current || tab === 'info');
  /** The empty, unfiltered timeline carries its own log buttons in the middle of the page; a
   *  second "+ Log" beside them would be two calls to one action. */
  const timelineHasEntries = $derived(activities.length > 0 || category !== '' || tagFilter !== null || titleFilter !== null);
```

3. Change the Info-loading effect from `if (tab === 'info')` to `if (detailsShown)`:

```ts
  $effect(() => { oid; if (detailsShown) { loadChildren(); loadLastDone(); if (offersTrip) loadTripSummary(); } });
```

   and update the comments on `loadChildren`, `lastDone` and `tripSummary`: "loaded only while
   the details are shown (Info tab, or the desktop pane)".

Template: replace everything from `<main>` to `</main>` with:

```svelte
<main>
  {#if error}<p class="error">{error}</p>{/if}
  {#if object}
    <TopBar title={object.name} icon={typeIcon(object.type, $customTypes)} backTo="/">
      {#if wide.current && (shownTab !== 'timeline' || timelineHasEntries)}
        <LogAction options={logChoices} onpick={(o) => go(o.path)} placement="header" />
      {/if}
      <!-- Icon only on a phone, icon and word from 1024 px; the name is "Edit" either way. -->
      <Button variant="outline" class="min-h-11 min-w-11 gap-1.5 px-3" aria-label={$t('nav.edit')} onclick={() => go(`/objects/${oid}/edit`)}>
        <Icon name="edit" size={18} /><span class="max-wide:hidden">{$t('nav.edit')}</span>
      </Button>
    </TopBar>

    {#if (object.ancestors ?? []).length > 0}
      <nav aria-label={$t('object.breadcrumb')} class="mb-2 text-sm text-muted-foreground">
        {#each object.ancestors ?? [] as a, i (a.id)}
          <a href={`/objects/${a.id}`}
             class="text-muted-foreground no-underline underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring"
             onclick={(e) => { e.preventDefault(); go(`/objects/${a.id}`); }}>{a.name}</a>{#if i < (object.ancestors ?? []).length - 1}<span aria-hidden="true"> › </span>{/if}
        {/each}
      </nav>
    {/if}

    <!-- One column on a phone, two panes from 1024 px. The summary is the same markup in both;
         the details are rendered once, in the pane or under Info (see `wide`). The pane is
         sticky and scrolls on its own: it is taller than the screen, and a sticky box taller
         than the screen would hide its end until the timeline ran out. `-mx-1 px-1` leaves
         room for focus rings, which a scroller clips. -->
    <div class="flex flex-col gap-4 wide:grid wide:grid-cols-[300px_minmax(0,1fr)] wide:items-start wide:gap-6">
      <section aria-label={$t('object.summary')}
               class="flex flex-col gap-4 wide:sticky wide:top-20 wide:-mx-1 wide:max-h-[calc(100dvh-6rem)] wide:overflow-y-auto wide:overscroll-contain wide:px-1 wide:pb-4">
        <ObjectSummary {object} {insights} due={dueReminders} onreminders={() => setTab('reminders')} />
        {#if wide.current}{@render details()}{/if}
      </section>

      <div class="flex min-w-0 flex-col gap-4">
        {#if object.type === 'body'}
          <WeightHistory objectId={oid} unit={object.weight_unit ?? 'kg'} />
        {/if}
        <Tabs.Root value={shownTab} onValueChange={(v) => setTab(v as Tab)}>
          <Tabs.List aria-label={$t('object.sections')}>
            <Tabs.Trigger value="timeline">{$t('tab.timeline')}</Tabs.Trigger>
            <Tabs.Trigger value="documents">{$t('tab.documents')}</Tabs.Trigger>
            <Tabs.Trigger value="reminders">
              {$t('tab.reminders')}
              {#if object.stats.due_reminder_count > 0}
                <span data-testid="tab-due-badge" aria-hidden="true"
                      class="inline-grid h-5 min-w-5 place-items-center rounded-full bg-destructive px-1.5 text-xs font-semibold text-destructive-foreground tabular-nums">{object.stats.due_reminder_count}</span>
                <span class="sr-only">{$t('tab.reminders-due', { n: object.stats.due_reminder_count })}</span>
              {/if}
            </Tabs.Trigger>
            {#if !wide.current}<Tabs.Trigger value="info">{$t('tab.info')}</Tabs.Trigger>{/if}
          </Tabs.List>

          <Tabs.Content value="timeline">
            {#if shownTab === 'timeline'}
              <Timeline
                objectId={oid} type={object.type} weightUnit={object.weight_unit} {activities} total={activityTotal} {loadingMore}
                onmore={loadMore} onlog={() => go(`/objects/${oid}/activities/new`)}
                ontriplog={offersTrip ? () => go(`/objects/${oid}/activities/new?category=trip`) : undefined}
                onchargelog={offersEnergy ? () => go(`/objects/${oid}/activities/new?category=${resourceCategory}`) : undefined}
                unit={object.counter_unit} fuelUnit={object.resource_unit ?? object.fuel_unit} resourceKind={object.resource_kind} energyRate={energyData?.cost_per_counter_milli ?? null}
                bind:category bind:tagFilter bind:titleFilter
              />
            {/if}
          </Tabs.Content>
          <Tabs.Content value="documents">
            {#if shownTab === 'documents'}<Documents objectId={oid} coverAttachmentId={object.cover_attachment_id} onchanged={loadObject} />{/if}
          </Tabs.Content>
          <Tabs.Content value="reminders">
            {#if shownTab === 'reminders'}
              <Reminders body={object.type === 'body'} objectId={oid} unit={object.counter_unit} {activities}
                         onchanged={() => { loadObject(); loadActivities('refresh'); if (dueCount > 0) loadDue(); }} />
            {/if}
          </Tabs.Content>
          {#if !wide.current}
            <Tabs.Content value="info">{#if shownTab === 'info'}{@render details()}{/if}</Tabs.Content>
          {/if}
        </Tabs.Root>
      </div>
    </div>

    <!-- Below 1024 px "+ Log" floats over the timeline; from 1024 px it is in the header above. -->
    {#if !wide.current && shownTab === 'timeline' && timelineHasEntries}
      <div class="fab-row">
        <LogAction options={logChoices} onpick={(o) => go(o.path)} />
      </div>
    {/if}
  {:else if !error}
    <p class="muted">{$t('nav.loading')}</p>
  {/if}
</main>

{#snippet details()}
  {#if object}
    <ObjectDetails
      {object} {insights} {insightsError} {lastDone} {tripSummary} energy={energyData} inside={children}
      {hasContents} contents={$includeContents} {offersTrip} {offersEnergy}
      oncontents={(on) => includeContents.set(on)} onlastdone={selectLastDone} onimported={() => loadActivities('refresh')}
    />
  {/if}
{/snippet}
```

In the `<style>` block, delete `.breadcrumb`, `.breadcrumb a`, `.desc` and `.info-actions`. Only
the `.fab-row` rules remain.

- [ ] **Step 7: `27-last-done.spec.ts` off `.card.entry`.** At desktop width the Last done rows
  and the timeline are on screen together, so the class matches both.
  - Lines 45, 51 and 81: `page.locator('.card.entry', { hasText: … })` becomes
    `page.getByTestId('last-done-row').filter({ hasText: … })`.
  - The assertions do not change.

- [ ] **Step 8: Verify, look, commit**

Run: `npm run check && npm test && npm run e2e`: all PASS on both projects. Capture
`../shots/r3-t4` and check:
- `05-object-timeline-desktop`: two panes, the pane 300 px, the header holding "+ Log" and Edit,
  nothing overlapping the sticky header.
- `07-object-info-desktop`: the same page, since Info redirects.
- mobile: unchanged from Task 3 apart from the header Edit button.
- In dark mode, the pane's cards separate from the page by surface shade.

```bash
git add -A frontend
git commit -m "feat: object page in two panes from 1024 px; Log and Edit in the header"
```

### Task 5: Timeline entries

**Files:**
- Create: `frontend/src/lib/CategoryIcon.svelte`
- Modify: `frontend/src/lib/Timeline.svelte`, `frontend/src/app.css`,
  `frontend/tests/theme-contrast.test.ts`, and the e2e specs that located entries by class:
  `04-offline`, `05-pagination`, `09-object-types`, `15-reading-reminders`, `18-templates`,
  `23-tags`, `27-last-done`, `28-trips`, `29-charging`
- Test: `frontend/tests-e2e/37-object-detail.spec.ts`

**Interfaces:**
- Produces: `<CategoryIcon category={Category} size?={number} />`. Timeline test ids:
  - `category-chips` (wraps the scroller);
  - `timeline-filter` (each active tag/title filter row);
  - `timeline-entry` (one per entry, the `<li>`);
  - `timeline-reading` (a reading row's button);
  - `timeline-fold` (a folded run's button);
  - `entry-thumbs`.
  Year headings are `<h2>`. Each entry's title is a button named by the title, which opens the
  entry; the whole card is its target.

- [ ] **Step 1: Failing e2e test.** Append to `37-object-detail.spec.ts`:

```ts
test('a timeline entry: icon, title, date and counter, amount on the right, tags inside, years as headings', async ({ page }, info) => {
  await signInFresh(page, '37-timeline');
  const car = await object(page, { name: 'Timeline car', type: 'car', counter_unit: 'km', fuel_unit: 'l' });
  await entry(page, car, { date: '2026-01-05', category: 'repair', title: 'Brake pads', counter_value: 45_800, cost_cents: 31_200, tags: ['service'] });
  await entry(page, car, { date: '2025-06-10', category: 'inspection', title: 'Inspection 2025', counter_value: 41_020, cost_cents: 5_000 });
  await page.goto(`/objects/${car}`);

  await expect(page.getByRole('heading', { name: '2026', exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: '2025', exact: true })).toBeVisible();
  const e = page.getByTestId('timeline-entry').filter({ hasText: 'Brake pads' });
  await expect(e.getByRole('img', { name: 'Repair' })).toBeVisible();
  await expect(e).toContainText('45,800 km');
  const box = (await e.boundingBox())!;
  const amount = (await e.getByText('€312.00').boundingBox())!;
  expect(box.x + box.width - (amount.x + amount.width)).toBeLessThan(24);
  const tag = (await e.getByRole('button', { name: 'service' }).boundingBox())!;
  expect(tag.y + tag.height).toBeLessThanOrEqual(box.y + box.height);

  // The category chips are one row that scrolls sideways on a phone.
  const chips = page.getByTestId('category-chips');
  const ys = await chips.getByRole('button').evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().y)));
  expect(new Set(ys).size).toBe(1);
  if (info.project.name === 'mobile') {
    expect(await chips.evaluate((el) => { const s = el.firstElementChild as HTMLElement; return s.scrollWidth > s.clientWidth; })).toBe(true);
  }

  // The tag filters; anywhere else on the card opens the entry.
  await e.getByRole('button', { name: 'service' }).click();
  await expect(page.getByTestId('timeline-filter')).toContainText('service');
  await page.getByTestId('timeline-entry').filter({ hasText: 'Brake pads' }).click({ position: { x: 8, y: 8 } });
  await expect(page).toHaveURL(new RegExp(`/objects/${car}/activities/\\d+$`));
});
```

Run: `npm run build && npx playwright test 37-object-detail -g "timeline entry"`. Expected: FAIL
(no `timeline-entry`).

- [ ] **Step 2: `frontend/src/lib/CategoryIcon.svelte`**

```svelte
<script lang="ts">
  import Activity from '@lucide/svelte/icons/activity';
  import CalendarClock from '@lucide/svelte/icons/calendar-clock';
  import CircleDot from '@lucide/svelte/icons/circle-dot';
  import ClipboardCheck from '@lucide/svelte/icons/clipboard-check';
  import Fuel from '@lucide/svelte/icons/fuel';
  import Gauge from '@lucide/svelte/icons/gauge';
  import Hammer from '@lucide/svelte/icons/hammer';
  import Pill from '@lucide/svelte/icons/pill';
  import Route from '@lucide/svelte/icons/route';
  import Scale from '@lucide/svelte/icons/scale';
  import Settings2 from '@lucide/svelte/icons/settings-2';
  import ShoppingCart from '@lucide/svelte/icons/shopping-cart';
  import Stethoscope from '@lucide/svelte/icons/stethoscope';
  import Thermometer from '@lucide/svelte/icons/thermometer';
  import Wrench from '@lucide/svelte/icons/wrench';
  import type { Category } from './types';

  /** An entry's category as a glyph. From lucide (already a dependency via the generated
   *  components) rather than Icon.svelte: these load only with the object page, while Icon.svelte
   *  is in the chunk every start loads. The Record is exhaustive, so a new category is a type
   *  error here until it has an icon. Decorative: the tile around it carries the name. */
  let { category, size = 18 }: { category: Category; size?: number } = $props();

  const ICONS: Record<Category, typeof Wrench> = {
    maintenance: Wrench, repair: Hammer, purchase: ShoppingCart, inspection: ClipboardCheck, modification: Settings2,
    fuel: Fuel, usage: Gauge, other: CircleDot, symptom: Thermometer, treatment: Stethoscope, appointment: CalendarClock,
    medication: Pill, reading: Gauge, trip: Route, weight: Scale, session: Activity,
  };
  const Glyph = $derived(ICONS[category] ?? CircleDot);
</script>

<Glyph {size} strokeWidth={1.75} aria-hidden="true" />
```

- [ ] **Step 3: Rewrite `Timeline.svelte`.** Keep the props block and the derived values
  (`chargeStem`, `groups`, `hasMore`, `present`, `chipCategories`, `open`, `toggle`) unchanged.
  Change the imports to the list below, add the helpers after `toggle`, replace the whole template,
  and delete the `<style>` block.

Imports (replacing the current list):

```ts
  import { formatWeight } from './weight';
  import type { WeightUnit } from './types';
  import { fileUrl } from './api';
  import { go } from './router';
  import { counter, fmtDate, money, quantity } from './format';
  import { dateFormat } from '../stores/date-format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import { activityTitle, groupByYear } from './activity-form';
  import { energyCost, energyLabelKey, fuelUnitLabel } from './energy';
  import { foldReadings, readingSpan } from './timeline-fold';
  import { placesLabel, spanLabel, tripDistance, formatDuration } from './trip';
  import { categoriesFor, customTypes } from './type-registry';
  import { CATEGORIES, type Activity, type Category, type CounterUnit, type ResourceUnit, type ObjectType } from './types';
  import { Button } from '$lib/components/ui/button/index.js';
  import CategoryIcon from './CategoryIcon.svelte';
  import Icon from './Icon.svelte';
  import TagChips from './TagChips.svelte';
  import { tagColorIndex } from './tags';
```

Helpers (after `toggle`):

```ts
  /** The entry's title: a weight entry is its weight, anything else its title or its category's
   *  fallback wording. */
  const title = (a: Activity) => (a.weight_grams != null ? formatWeight(a.weight_grams, weightUnit, $locale) : activityTitle(a.title, a.category, $t, fuelUnit));

  /** The line under an entry's title: the date, then what the entry measured, joined by " · ".
   *  - a trip: the counter span it covered, its length, and its estimated energy cost (never
   *    stored; the "≈" tells it apart from a recorded amount);
   *  - a fill or charge: the counter, "full", the amount, a meter reading, "estimated", the level;
   *  - anything else: the counter.
   *  The amount paid is not here: it stands at the right of the title, where a column of entries
   *  lines their amounts up. */
  function meta(a: Activity): string {
    const q = (m: number | null | undefined) => quantity(m, fuelUnit ? fuelUnitLabel(fuelUnit) : null, $locale);
    const parts: string[] = [fmtDate(a.date, $dateFormat)];
    if (a.category === 'trip') {
      const dist = tripDistance(a);
      parts.push(spanLabel(counter(a.start_counter, unit, $locale), counter(a.counter_value, unit, $locale)));
      if (dist !== null) {
        parts.push(counter(dist, unit, $locale));
        if (energyRate !== null) parts.push(`≈ ${energyCost(dist, energyRate, $currency, $locale)}`);
      }
      return parts.join(' · ');
    }
    if (a.counter_value !== null) parts.push(counter(a.counter_value, unit, $locale));
    if (a.category === 'fuel' || a.category === 'usage') {
      if (a.charged_full) parts.push($t('energy.full'));
      if (a.quantity_milli !== null) parts.push(q(a.quantity_milli));
      if (a.meter_reading_milli != null) parts.push(`${$t('water.meter-reading')}: ${q(a.meter_reading_milli)}`);
      if (a.estimated) parts.push($t('water.estimated-short'));
      if (a.fuel_level_pct != null) parts.push($t('activity.fuel-level-value', { pct: a.fuel_level_pct }));
    }
    return parts.join(' · ');
  }

  const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring';
  /** Stretched link, as on the object cards: the title's ::after covers the entry. */
  const stretched = "after:absolute after:inset-0 after:rounded-lg after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-solid focus-visible:after:outline-offset-2 focus-visible:after:outline-ring";
  const readingCls = 'flex min-h-11 w-full cursor-pointer items-baseline justify-between gap-2 rounded-md border border-dashed border-border bg-transparent px-3 py-2 text-left text-sm text-foreground disabled:cursor-default disabled:opacity-60';
```

Template:

```svelte
{#snippet readingRow(a: Activity)}
  <!-- A reading is one number, so it is one line, not a card. Still a button, so a typo can be
       opened and fixed like any other entry. -->
  <button data-slot="timeline-reading" data-testid="timeline-reading" disabled={a.pending} class={`${readingCls} ${focus}`}
          onclick={() => go(`/objects/${objectId}/activities/${a.id}`)}>
    <span class="text-muted-foreground">{fmtDate(a.date, $dateFormat)} · {$t('cat.reading')}{#if a.pending} · {$t('timeline.pending')}{/if}</span>
    <span class="tabular-nums">{counter(a.counter_value, unit, $locale)}</span>
  </button>
{/snippet}

{#snippet chip(value: Category | '', label: string)}
  <!-- 36 px to look at, 44 px to hit: the ::before adds 4 px above and below, inside the
       scroller's padding so it is not clipped. -->
  <button data-slot="filter-chip" aria-pressed={category === value} onclick={() => (category = value)}
          class={["relative inline-flex h-9 shrink-0 cursor-pointer items-center whitespace-nowrap rounded-full border px-3 text-sm font-medium transition-colors before:absolute before:inset-x-0 before:-inset-y-1 before:content-['']", focus,
                  category === value ? 'border-transparent bg-primary text-primary-foreground' : 'border-border bg-card text-foreground hover:bg-muted']}>{label}</button>
{/snippet}

<!-- The chips are one row that scrolls sideways; the fade at the right edge says there is more.
     The 4 px padding is the focus ring's bleed (2 px outline + 2 px offset), which the scroller
     would otherwise clip, and the fade is dropped while a chip has focus so it does not dim the
     ring. -->
<div data-testid="category-chips" class="-mx-1 mb-3">
  <div class="flex gap-2 overflow-x-auto p-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden [mask-image:linear-gradient(to_right,#000_calc(100%_-_24px),transparent)] focus-within:[mask-image:none]">
    {@render chip('', $t('timeline.filter-all'))}
    {#each chipCategories as c (c)}{@render chip(c, $t(`cat.${c}`))}{/each}
  </div>
</div>

{#if tagFilter !== null}
  <div data-testid="timeline-filter" class="mb-3 flex flex-wrap items-center gap-2">
    <span class={`tag tag-${tagColorIndex(tagFilter)}`}>{$t('tags.filter', { tag: tagFilter })}</span>
    <Button variant="outline" class="min-h-11" onclick={() => (tagFilter = null)}>{$t('tags.clear')}</Button>
  </div>
{/if}

{#if titleFilter !== null}
  <!-- Same pattern as the tag filter, combinable with it and with the category: this one narrows
       by exact title (a "Last done" row tapped in the details). `titleFilter` stays the real value
       the server matches on; only the chip's text falls back for an untitled entry. -->
  <div data-testid="timeline-filter" class="mb-3 flex flex-wrap items-center gap-2">
    <span class="rounded-full bg-muted px-3 py-1 text-sm text-foreground">{$t('lastdone.filter', { title: activityTitle(titleFilter, undefined, $t) })}</span>
    <Button variant="outline" class="min-h-11" onclick={() => (titleFilter = null)}>{$t('lastdone.clear')}</Button>
  </div>
{/if}

{#if activities.length === 0}
  <!-- An object with no history and an object whose filter matched nothing are not the same
       screen: the first is an invitation, the second is a fact about the chips above. -->
  <div class="flex flex-col items-center gap-3 px-4 py-10 text-center">
    {#if category === '' && tagFilter === null && titleFilter === null}
      <span class="text-muted-foreground opacity-40"><Icon name="edit" size={40} /></span>
      <p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('timeline.empty')}</p>
      {#if onlog}<Button class="min-h-11" onclick={() => onlog()}>+ {$t('timeline.log')}</Button>{/if}
      {#if ontriplog}<Button variant="outline" class="min-h-11" onclick={() => ontriplog()}>+ {$t('trip.log')}</Button>{/if}
      {#if onchargelog}<Button variant="outline" class="min-h-11" onclick={() => onchargelog()}>+ {resourceKind === 'water' ? $t('water.log') : $t(`${chargeStem}-log`)}</Button>{/if}
    {:else}
      <p class="m-0 text-sm text-muted-foreground">{$t('timeline.none-in-filter')}</p>
    {/if}
  </div>
{:else}
  {#each groups as [year, rows], gi (year)}
    <h2 class={['mb-2 text-sm font-semibold text-muted-foreground tabular-nums', gi === 0 ? 'mt-1' : 'mt-5']}>{year}</h2>
    <ul role="list" class="m-0 flex list-none flex-col gap-2 p-0">
      {#each rows as row (row.kind === 'entry' ? row.activity.id : row.key)}
        {#if row.kind === 'readings'}
          {@const span = readingSpan(row.readings)}
          {@const expanded = open.includes(row.key)}
          <!-- A run of readings between two real entries says one thing -- the counter went from
               here to there -- so it is one line until someone asks for the detail. -->
          <li>
            <button data-slot="timeline-fold" data-testid="timeline-fold" aria-expanded={expanded} onclick={() => toggle(row.key)} class={`${readingCls} ${focus}`}>
              <span class="text-muted-foreground">
                <span class="inline-block w-4" aria-hidden="true">{expanded ? '▾' : '▸'}</span>
                {fmtDate(row.readings[row.readings.length - 1].date, $dateFormat)} – {fmtDate(row.readings[0].date, $dateFormat)}
                · {$t('timeline.readings', { n: row.readings.length })}
              </span>
              {#if span}<span class="tabular-nums">{counter(span.from, unit, $locale)} – {counter(span.to, unit, $locale)}</span>{/if}
            </button>
            {#if expanded}
              <ul role="list" class="m-0 mt-2 flex list-none flex-col gap-2 p-0 pl-4">
                {#each row.readings as a (a.id)}<li>{@render readingRow(a)}</li>{/each}
              </ul>
            {/if}
          </li>
        {:else if row.activity.category === 'reading'}
          {@const a = row.activity}
          <!-- A single reading shows its chips like any entry; a folded run stays one line each. -->
          <li class="flex flex-col gap-1">
            {@render readingRow(a)}
            {#if (a.tags ?? []).length > 0}
              <div class="w-fit pl-3"><TagChips tags={a.tags} onselect={(tag) => (tagFilter = tag)} active={tagFilter} /></div>
            {/if}
          </li>
        {:else}
          {@const a = row.activity}
          <li data-testid="timeline-entry"
              class={['relative flex gap-3 rounded-lg border border-border bg-card p-3 shadow-xs transition-colors hover:border-input', a.pending && 'opacity-60']}>
            <span role="img" aria-label={$t(`cat.${a.category}`)} class="grid size-10 shrink-0 place-items-center rounded-md bg-primary/10 text-brand-ink">
              <CategoryIcon category={a.category} />
            </span>
            <div class="flex min-w-0 flex-1 flex-col gap-0.5">
              <div class="flex items-baseline gap-2">
                <button data-slot="entry-open" disabled={a.pending} onclick={() => go(`/objects/${objectId}/activities/${a.id}`)}
                        class={`min-w-0 flex-1 cursor-pointer break-words text-left font-semibold text-foreground disabled:cursor-default ${stretched}`}>{title(a)}</button>
                {#if a.cost_cents !== null}<span class="shrink-0 font-semibold text-foreground tabular-nums">{money(a.cost_cents, $currency, $locale)}</span>{/if}
              </div>
              <p class="m-0 text-sm text-muted-foreground tabular-nums">{meta(a)}</p>
              {#if a.category === 'trip' && placesLabel(a.from_place, a.to_place)}
                <p class="m-0 text-sm text-muted-foreground">{placesLabel(a.from_place, a.to_place)}</p>
              {/if}
              {#if a.category === 'trip' && (a.duration_minutes !== null || a.battery_used_pct !== null)}
                <p class="m-0 text-sm text-muted-foreground tabular-nums">
                  {[
                    a.duration_minutes !== null ? `${formatDuration(a.duration_minutes)} h` : null,
                    a.battery_used_pct !== null ? `${a.battery_used_pct} %` : null,
                  ].filter((s) => s !== null).join(' · ')}
                </p>
              {/if}
              {#if a.pending}<span class="w-fit rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('timeline.pending')}</span>{/if}
              {#if a.notes}<p class="m-0 mt-1 whitespace-pre-wrap text-sm text-foreground">{a.notes}</p>{/if}
              {#if a.attachments.length > 0}
                <!-- Wraps rather than scrolls: the stretched target covers this row, so a sideways
                     swipe here would never reach a scroller. -->
                <div data-testid="entry-thumbs" class="mt-1 flex flex-wrap gap-2">
                  {#each a.attachments.slice(0, 6) as att (att.id)}
                    {#if att.kind === 'photo'}
                      <img class="block size-16 shrink-0 rounded-md object-cover" src={fileUrl(att.file_id, true)} alt="" loading="lazy" decoding="async" />
                    {:else}
                      <span class="grid size-16 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground"><Icon name="document" size={28} /></span>
                    {/if}
                  {/each}
                </div>
              {/if}
              {#if (a.tags ?? []).length > 0}
                <!-- The chips are buttons of their own, raised above the stretched target. -->
                <div class="relative z-10 mt-1 w-fit"><TagChips tags={a.tags} onselect={(tag) => (tagFilter = tag)} active={tagFilter} /></div>
              {/if}
            </div>
          </li>
        {/if}
      {/each}
    </ul>
  {/each}
  {#if hasMore}
    <Button variant="outline" class="mt-3 min-h-11 w-full" onclick={() => onmore?.()} disabled={loadingMore}>
      {loadingMore ? $t('nav.loading') : $t('timeline.more', { n: total - activities.length })}
    </Button>
  {/if}
{/if}
```

If svelte-check rejects `category = value` inside the snippet (a bindable prop assigned from a
snippet), move it into `function pick(v: Category | '') { category = v; }` and call `pick(value)`.

- [ ] **Step 4: app.css.** Delete the `.year { … }` rule (`grep -rn '"year"' src` prints nothing
  now). `.chips` and `.thumb-strip` stay, because ActivityForm and ObjectForm still use them
  (round 4).

- [ ] **Step 5: Contrast note.** In `theme-contrast.test.ts`, rename
  `'brand-ink on the object card icon tile (primary/10 over card)'` to
  `'brand-ink on an icon tile -- object card, timeline entry (primary/10 over card)'`. The pair is
  the same; the test now names both users.

- [ ] **Step 6: Move the specs off the timeline's classes.** Each assertion keeps its meaning;
  only the locator changes.
  - `05-pagination.spec.ts:32,86`: `page.locator('button.entry')` becomes
    `page.getByTestId('timeline-entry')`.
  - `15-reading-reminders.spec.ts:38` and `18-templates.spec.ts:24`: `page.locator('.entry.reading')`
    becomes `page.getByTestId('timeline-reading')`.
  - `09-object-types.spec.ts:52,75`: `page.locator('.chips button')` becomes
    `page.getByTestId('category-chips').getByRole('button')`.
  - `23-tags.spec.ts`:
    - line 14–16: the helper's comment becomes "The entry's chips on the timeline: inside its
      `timeline-entry`", and its body becomes
      `return page.getByTestId('timeline-entry').filter({ hasText: title }).locator('.tag', { hasText: tag });`.
    - line 41: `page.getByTestId('timeline-entry').filter({ hasText: 'Bremsbeläge vorne' }).getByRole('button', { name: 'Bremsbeläge vorne' }).click()`.
    - line 83: `expect(page.getByTestId('timeline-entry').filter({ hasText: 'Bremsbeläge hinten' }).getByText('Waiting to send')).toHaveCount(0)`.
    - line 137–138: the comment becomes "an entry's chips are inside its `timeline-entry`", and the
      locator becomes `page.getByTestId('timeline-entry').locator('.tag', { hasText: 'Winter' }).first()`.
    - line 239: `page.locator('.tag-filter', { hasText: 'Antrieb' })` becomes
      `page.getByTestId('timeline-filter').filter({ hasText: 'Antrieb' })`.
  - `27-last-done.spec.ts`:
    - lines 56 and 82: `.tag-filter` becomes `getByTestId('timeline-filter').filter({ hasText: … })`.
    - lines 64 and 92: `page.locator('.tag-filter')` becomes `page.getByTestId('timeline-filter')`.
    - lines 57, 59, 60, 65 and 93: `page.locator('.entry-row'…)` becomes
      `page.getByTestId('timeline-entry')`, with `.filter({ hasText })` where the old call passed
      `hasText`.
  - `28-trips.spec.ts:48` (`.card.entry`) and `28-trips.spec.ts:77` (`.entry-row`) become
    `page.getByTestId('timeline-entry').filter({ hasText: … })` and
    `page.getByTestId('timeline-entry')`.
  - `29-charging.spec.ts:40,86,97`: `page.locator('.card.entry', { hasText: X })` becomes
    `page.getByTestId('timeline-entry').filter({ hasText: X })`.
  - `04-offline.spec.ts:124`: `otherPage.locator('.thumb-strip img')` becomes
    `otherPage.getByTestId('entry-thumbs').locator('img')`.
    Lines 86 and 95 (`.thumb-strip .strip-item`) are ActivityForm's strip, which is round 4's
    screen, and stay.

- [ ] **Step 7: Verify, look, commit**

Run: `npm run check && npm test && npm run e2e`: all PASS. Capture `../shots/r3-t5` and check,
light and dark, mobile and desktop:
- the icon tiles are amber-tinted and readable;
- the amounts line up on the right;
- the "service" tags sit inside the entries;
- the year headings are muted;
- the chips row fades at the right edge on a phone and does not wrap.

```bash
git add -A frontend
git commit -m "feat: timeline entries with category icon, amount on the right and tags inside"
```

### Task 6: Reminders and Documents tabs

**Files:**
- Modify: `frontend/src/lib/Reminders.svelte`, `frontend/src/lib/Documents.svelte`,
  `frontend/src/app.css`, `frontend/tests/theme-contrast.test.ts`,
  `frontend/tests-e2e/02-lifecycle.spec.ts`, `15-reading-reminders.spec.ts`, `18-templates.spec.ts`,
  `23-tags.spec.ts`, `32-calendar-reminders.spec.ts`
- Test: `frontend/tests-e2e/37-object-detail.spec.ts`

**Interfaces:**
- Produces:
  - `data-testid="reminder-card"` (each open or due reminder, an `<li>`), `reminder-status` (its
    status badge: "Due", "Open" or "Snoozed") and `reminder-done` (a done one in the history).
  - `document` (each file in the Documents grid).
  - Button names are unchanged: Edit, Mark done, Record reading, Skip this one, Unsnooze,
    Use as cover, Remove cover, Delete.

- [ ] **Step 1: Failing e2e test.** Append to `37-object-detail.spec.ts`:

```ts
test('a due reminder card on the Reminders tab says so, and its actions are full-size', async ({ page }) => {
  await signInFresh(page, '37-reminders');
  const car = await object(page, { name: 'Reminder tab car', type: 'car', counter_unit: 'km' });
  for (const data of [{ title: 'Tab overdue', due_date: '2000-01-01' }, { title: 'Tab later', due_date: '2099-01-01' }]) {
    expect((await page.request.post(`/api/objects/${car}/reminders`, { data })).ok()).toBe(true);
  }
  await page.goto(`/objects/${car}?tab=reminders`);

  const due = page.getByTestId('reminder-card').filter({ hasText: 'Tab overdue' });
  await expect(due.getByTestId('reminder-status')).toHaveText('Due');
  await expect(page.getByTestId('reminder-card').filter({ hasText: 'Tab later' }).getByTestId('reminder-status')).toHaveText('Open');
  for (const name of ['Edit', 'Mark done']) {
    expect((await due.getByRole('button', { name }).boundingBox())!.height).toBeGreaterThanOrEqual(44);
  }
});
```

Run: `npm run build && npx playwright test 37-object-detail -g "Reminders tab"`. Expected: FAIL.

- [ ] **Step 2: `Reminders.svelte`.**
  - Add `import { Button } from '$lib/components/ui/button/index.js';`.
  - Replace everything from `{#if error}` down to just before `<!-- The empty state carries this
    same action …` with the markup below.
  - Leave the floating `+ New reminder` button and the `<dialog>` as they are (app.css's `.fab`
    and `dialog`, until rounds 4/5).
  - Delete the whole `<style>` block.

```svelte
{#if error}<p class="error" role="alert">{error}</p>{/if}
{#if toast}<p class="m-0 mb-3 text-sm text-muted-foreground">{toast}</p>{/if}

<!-- Not `items.length === 0`: an empty list before the first answer is what the component was
     initialised with, not what the server said. -->
{#if loaded && items.length === 0}
  <div class="flex flex-col items-center gap-3 px-4 py-10 text-center">
    <span class="text-muted-foreground opacity-40"><Icon name="repeat" size={40} /></span>
    <p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('reminder.empty')}</p>
    <Button class="min-h-11" onclick={() => go(`/objects/${objectId}/reminders/new`)}>+ {$t('reminder.new')}</Button>
    {#if unit || body}
      <Button variant="outline" class="min-h-11" onclick={() => go(`/objects/${objectId}/reminders/new?kind=reading`)}>{$t(body ? 'weight.reminder' : 'reminder.new-reading')}</Button>
    {/if}
  </div>
{/if}

<!-- A due reminder's card is tinted like the dashboard's, so what needs doing reads first. -->
<ul role="list" class="m-0 flex list-none flex-col gap-2 p-0">
  {#each [...groups.due, ...groups.open] as r (r.id)}
    <li data-testid="reminder-card"
        class={['flex flex-col gap-1 rounded-lg border p-3 shadow-xs', r.due ? 'border-destructive/30 bg-destructive/10' : 'border-border bg-card']}>
      <div class="flex items-start gap-2">
        <span class="min-w-0 flex-1 break-words font-semibold text-foreground">{r.title}</span>
        {#if r.pending}
          <!-- Only in the outbox so far: the server has not computed whether it is due. -->
          <span class="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('timeline.pending')}</span>
        {:else}
          <span data-testid="reminder-status"
                class={['shrink-0 rounded-full px-2 py-0.5 text-xs font-semibold', r.due ? 'bg-destructive text-destructive-foreground' : 'bg-muted text-muted-foreground']}>
            {r.due ? $t('reminder.due') : r.snoozed_until ? $t('reminder.snoozed') : $t('reminder.open')}
          </span>
        {/if}
      </div>
      {#if (r.object_tags ?? []).length > 0}<div class="w-fit"><TagChips tags={r.object_tags ?? []} /></div>{/if}
      {#if r.kind === 'reading'}
        <p class="m-0 flex items-center gap-1 text-sm text-muted-foreground"><span class="inline-flex" role="img" aria-label={$t('activity.repeat')}><Icon name="repeat" size={14} /></span> {every(r)}</p>
        <p class="m-0 text-sm text-muted-foreground tabular-nums">{reading(r)}</p>
      {:else}
        <p class="m-0 text-sm text-muted-foreground">
          {when(r)}{#if r.repeat_months || r.repeat_counter}{' · '}<span class="inline-flex align-middle" role="img" aria-label={$t('activity.repeat')}><Icon name="repeat" size={14} /></span>{/if}
        </p>
        {#if r.estimated_due_date}
          <p class="m-0 text-sm text-muted-foreground">{$t('reminder.estimated', { date: fmtDate(r.estimated_due_date, $dateFormat) })}</p>
        {/if}
      {/if}
      {#if !r.due && r.snoozed_until}
        <!-- `due_date`/`due_counter` never change on snooze (see src/api/reminders.rs), so the line
             above can still read as overdue while the reminder is suppressed -- this one says so. -->
        <div class="mt-1 flex items-center gap-2">
          <span class="min-w-0 flex-1 text-sm text-muted-foreground">{$t('reminder.snoozed-until', { date: fmtDate(r.snoozed_until, $dateFormat) })}</span>
          <Button variant="outline" class="min-h-11 shrink-0" onclick={() => unsnooze(r)}>{$t('reminder.unsnooze')}</Button>
        </div>
      {/if}
      {#if r.notes}<p class="m-0 mt-1 whitespace-pre-wrap text-sm text-foreground">{r.notes}</p>{/if}
      <!-- A pending reminder has no server row yet, so nothing here could address it. -->
      {#if !r.pending}
        <div class="mt-2 flex flex-wrap gap-2">
          <Button variant="outline" class="min-h-11" onclick={() => go(`/objects/${objectId}/reminders/${r.id}`)}>{$t('nav.edit')}</Button>
          {#if r.kind === 'reading'}
            <!-- No "done": logging the reading is what satisfies it, from here or anywhere else. -->
            {#if r.due}<Button variant="outline" class="min-h-11" onclick={() => skip(r)}>{$t('reminder.skip')}</Button>{/if}
            <Button class="min-h-11" onclick={() => go(`/objects/${objectId}/reading`)}>{$t(body ? 'weight.log' : 'reminder.record')}</Button>
          {:else}
            <Button class="min-h-11" onclick={() => openDone(r)}>{$t('reminder.mark-done')}</Button>
          {/if}
        </div>
      {/if}
    </li>
  {/each}
</ul>

{#if groups.done.length > 0}
  <Button variant="ghost" class="mt-4 min-h-11 w-full justify-start text-muted-foreground" aria-expanded={showDone} onclick={() => (showDone = !showDone)}>
    {showDone ? '▾' : '▸'} {$t('reminder.history')} ({groups.done.length})
  </Button>
  {#if showDone}
    <!-- Muted fill rather than faded opacity: faded text would drop below 4.5:1. -->
    <ul role="list" class="m-0 mt-2 flex list-none flex-col gap-2 p-0">
      {#each groups.done as r (r.id)}
        <li data-testid="reminder-done" class="flex flex-col gap-0.5 rounded-lg bg-muted p-3">
          <span class="font-semibold text-foreground">{r.title}</span>
          <span class="text-sm text-muted-foreground">{$t('reminder.done')} · {fmtDate(r.done_at, $dateFormat)}</span>
        </li>
      {/each}
    </ul>
  {/if}
{/if}
```

- [ ] **Step 3: `Documents.svelte`.**
  - Add `import { Button } from '$lib/components/ui/button/index.js';`.
  - Add a
    `const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring';`
    at the end of the script.
  - Replace everything after `<FilePicker … />` with the markup below, and delete the `<style>`
    block.

```svelte
{#if error}<p class="error" role="alert">{error}</p>{/if}

{#if !loaded}
  <!-- Nothing: the request is still out. -->
{:else if items.length === 0}
  <!-- The picker sits right above this, so the words only have to say what is worth putting
       into it. Not after a failed load: the error above already says why there is no list. -->
  {#if !error}
    <div class="flex flex-col items-center gap-3 px-4 py-10 text-center">
      <span class="text-muted-foreground opacity-40"><Icon name="document" size={40} /></span>
      <p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('docs.empty')}</p>
    </div>
  {/if}
{:else}
  <ul role="list" class="m-0 mt-3 grid list-none grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-3 p-0">
    {#each items as a (a.id)}
      <li data-testid="document" class="flex min-w-0 flex-col gap-1">
        {#if a.kind === 'photo'}
          <a href={fileUrl(a.file_id)} target="_blank" rel="noopener" class={`block rounded-md ${focus}`}>
            <img class="block aspect-square w-full rounded-md bg-muted object-cover" src={fileUrl(a.file_id, true)} alt={a.caption || a.original_name} loading="lazy" decoding="async" />
          </a>
        {:else}
          <a href={fileUrl(a.file_id)} target="_blank" rel="noopener" aria-label={a.caption || a.original_name}
             class={`grid aspect-square w-full place-items-center rounded-md bg-muted text-muted-foreground ${focus}`}><Icon name="document" size={32} /></a>
        {/if}
        <span class="truncate text-xs text-foreground">{a.caption || a.original_name}</span>
        <div class="flex flex-wrap gap-1">
          {#if a.kind === 'photo'}
            <Button variant="ghost" class="min-h-11 px-2 text-xs" onclick={() => setCover(a.id === coverAttachmentId ? null : a.id)}>
              {a.id === coverAttachmentId ? $t('object.clear-cover') : $t('object.set-cover')}
            </Button>
          {/if}
          <Button variant="ghost" class="min-h-11 px-2 text-xs text-destructive hover:text-destructive" onclick={() => remove(a)}>{$t('nav.delete')}</Button>
        </div>
      </li>
    {/each}
  </ul>
{/if}
```

- [ ] **Step 4: app.css.**
  - Delete `.chip.due { … }`. Its last users were the old tab badge (Task 1) and the old reminder
    status chip.
  - Delete `.thumb-grid { … }` with its comment, `img.thumb { … }` with its comment, and
    `.doc-icon { … }`.
  - Before deleting, run
    `grep -rn 'chip due\|class:due\|thumb-grid\|class="thumb\|doc-icon' src`; it must print nothing.
  - In `theme-contrast.test.ts`, the loop
    `for (const chip of ['.chip.due', '.chip.pending', '.chip.dead'])` becomes
    `for (const chip of ['.chip.pending', '.chip.dead'])`. The rule is gone; the due badge's pair
    (`destructive-foreground` on `destructive`) is already in the shadcn token list.

- [ ] **Step 5: Contrast test for the outline buttons on a due card.** Add to the
  `'tinted highlight contrast'` loop in `theme-contrast.test.ts`:

```ts
    // An outline button on a due reminder card: in dark mode its fill is input/30 painted over
    // the card's destructive/10 over the page. Light mode's is plain background; the stricter
    // blend is checked for both.
    it(`${name}: text on an outline button inside a due card (>= 4.5:1)`, () => {
      const card = blend(ui('destructive', theme), ui('background', theme), 0.1);
      const button = blend(ui('input', theme), card, 0.3);
      expect(contrastRatio(ui('foreground', theme), button)).toBeGreaterThanOrEqual(4.5);
    });
```

- [ ] **Step 6: Move the specs off `.card`, `.chip.due` and `.thumb-grid`**
  - `02-lifecycle.spec.ts:42`: `page.locator('.thumb-grid img.thumb')` becomes
    `page.getByTestId('document').locator('img')`.
  - `02-lifecycle.spec.ts:52`: `page.locator('.chip.due').first()` becomes
    `page.getByTestId('reminder-status').filter({ hasText: 'Due' }).first()`.
  - `15-reading-reminders.spec.ts:27,82`: `page.locator('.card').filter({ hasText: … })` becomes
    `page.getByTestId('reminder-card').filter({ hasText: … })`.
  - `15-reading-reminders.spec.ts:28`: `card.locator('.chip.due')).toBeVisible()` becomes
    `card.getByTestId('reminder-status')).toHaveText('Due')`.
  - `15-reading-reminders.spec.ts:42,85`: `card.locator('.chip.due')).toHaveCount(0)` becomes
    `card.getByTestId('reminder-status')).not.toHaveText('Due')`.
  - `18-templates.spec.ts:28,30,31` and `32-calendar-reminders.spec.ts:41,89`:
    `page.locator('.card').filter(…)` becomes `page.getByTestId('reminder-card').filter(…)`.
  - `23-tags.spec.ts:266`: `page.locator('.card', { hasText: 'Kette prüfen' })` becomes
    `page.getByTestId('reminder-card').filter({ hasText: 'Kette prüfen' })`.

- [ ] **Step 7: Verify, look, commit**

Run: `npm run check && npm test && npm run e2e`: all PASS. Capture `../shots/r3-t6` and check:
- `06-object-reminders`: the overdue "Winter tyres on" card is tinted with a solid "Due" badge;
  "Oil change" is plain with "Open"; the buttons are full-size and readable in dark mode.
- The Documents tab shows the grid, if the seed has any files; otherwise open one by hand once.

```bash
git add -A frontend
git commit -m "feat: reminders and documents tabs in the new style"
```

### Task 7: Budget check and release 0.21.0

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `frontend/package.json`, `frontend/package-lock.json`,
  `docs/openapi.json`, `docs/upgrading.md`

- [ ] **Step 1: Measure**

```bash
cd frontend && npm run build && for f in dist/assets/*.js dist/assets/*.css; do printf '%s %s\n' "$(gzip -9c "$f" | wc -c)" "$f"; done > ../.superpowers/sdd/bundle-r3-after.txt; cd ..
for f in before after; do awk -v n=$f '/\.js$/ {s+=$1} END {print n, "js", s}' .superpowers/sdd/bundle-r3-$f.txt; done
grep 'index-' .superpowers/sdd/bundle-r3-before.txt .superpowers/sdd/bundle-r3-after.txt
grep -l "bits-ui\|floating-ui\|lucide" frontend/dist/assets/index-*.js
```

Expected:
- The JS total grows by ≤ 10240 bytes.
- The `index-*.js` entry chunk grows by ≤ 200 bytes.
- The last `grep -l` prints nothing.

If any limit is exceeded, report the numbers and the biggest contributors instead of bumping. The
likely ones are bits-ui Tabs, the 15 lucide icons, and `ObjectDetails`/`Chart`. Compare the
`ObjectDetail-*.js` chunks before and after, and check that the lucide icons were tree-shaken.

- [ ] **Step 2: Bump to 0.21.0** in the six files, as round 2's Task 5 did (from 0.20.0). Add this
  above `## 0.20.0` in `docs/upgrading.md`:

```markdown
## 0.21.0: new look, the object page

**On a wide screen (1024 px and up) an object's page has two panes.** The left one holds the
photo, the key figures, anything due, spend per month and everything the Info tab used to show,
and it stays in view while the timeline scrolls. "+ Log" and Edit sit in the page header. There
is no Info tab at that width; an old `?tab=info` link opens the timeline beside the pane.

**On a phone** the photo, a row of figures and anything due sit above the tabs. The tabs stay on
one row, the Reminders count sits inside its tab, and Info holds the rest.

**Timeline entries** show a category icon, the amount on the right and their tags inside the
entry, under year headings; the category chips scroll sideways. Charts leave out empty months.
The Energy figures now show only for objects that charge (kWh), so a diesel car no longer shows
"Distance per charge".

No migration.
```

- [ ] **Step 3: Verify and commit**

```bash
cargo clippy --all-targets -- -D warnings
cargo test --test it openapi::
(cd frontend && npm run check && npm test && npm run e2e)
git add Cargo.toml Cargo.lock frontend/package.json frontend/package-lock.json docs/openapi.json docs/upgrading.md
git commit -m "chore: release 0.21.0"
```

Report:
- the bundle deltas: JS total, entry chunk, and the object chunk;
- the test counts;
- every changed e2e assertion: the 28-trips bar count, the 24-own-types type line, and the
  27-last-done `aria-selected`;
- before/after object-page screenshots, `shots/r3-before` against `shots/r3-t6`.

Stop there: merge, tag and push are the user's decision.
