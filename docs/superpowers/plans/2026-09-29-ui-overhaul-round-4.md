# UI overhaul, round 4 (forms): implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the four forms (activity, object, reminder, reading) and the inputs they share
in the new look. Every field has the same shape: label, control, hint, warning, error. Required
and value-carrying fields come first; the rest wait in a "More details" section that opens by
itself when the entry being edited uses any of them. Save and Cancel sit in a bar that sticks to
the bottom of the screen, above the phone's tab bar, so Save is visible when the form opens. The
reminder form asks "Due by: Date / Counter / Both" instead of showing two ambiguous fields. The
object form starts with the type as icon tiles, nothing preselected, then the templates under a
label, then the name. The activity form has one "Notes" label and small section headings. The
object-page leftovers from round 3 are cleaned up. Release as 0.22.0.

**Architecture:** Hand-written shadcn-style components in `frontend/src/lib/components/ui/`
(`field`, `input`, `textarea`, `native-select`, `checkbox`, `segmented`), plus three app pieces
in `frontend/src/lib/` (`FormActions`, `MoreDetails`, `TypeTiles`). A `Field` passes its id,
`aria-describedby` and `aria-invalid` to the control inside it through Svelte context, so call
sites name the id once. Validators keep returning an i18n key; a per-form map turns the key into
the id of the field, and the message is shown under that field (`role="alert"`) with the focus
moved there. All of it is imported only by the lazily loaded form routes and the object page.

**Tech Stack:** Svelte 5, Tailwind v4 (tokens in `frontend/src/app.tw.css`), bits-ui 2 (Checkbox
only), @lucide/svelte, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-29-ui-overhaul-design.md` (Rules for every round, Round 4
including the notes for rounds 4/5)

## Decisions (refinements of the spec, for the controller to relay)

1. **Selects stay native, styled (`NativeSelect`).** The spec says native selects are replaced by
   shadcn components. shadcn ships a `native-select` component for exactly this case, and it is
   the better choice here:
   - Phones keep the OS picker; long lists (31 days of the month, every object as "Inside") keep
     typeahead. A bits-ui Select is a popover listbox that is worse for both.
   - About 40 e2e calls use `selectOption`; they keep working unchanged.
   - No JavaScript: the forms' budget goes to the pieces that need it.
   The wrapper draws the box, the chevron (lucide) and the focus ring; the `<select>` stays real.
2. **Checkboxes become the shadcn Checkbox (bits-ui).** Checked is `brand-ink` with a
   `background`-coloured tick, not the amber fill: amber on white is 2.1:1, too faint for the
   box's edge (1.4.11). `.check()`/`toBeChecked()` and `getByLabel` keep working (`role="checkbox"`,
   `<label for>`).
3. **Radios become a segmented control and icon tiles on native radio inputs**, not bits-ui
   RadioGroup/ToggleGroup. The native group already gives arrow keys, one tab stop, form
   semantics and `getByRole('radio')`; the invisible input fills its label, which carries the
   look. Used for the reminder's kind and "Due by", and the object type tiles.
4. **"Required first" means required plus the fields that carry the entry's value.** Strictly
   required alone would hide Cost and the counter reading, which are what most entries are for.
   Per form, what stays in view and what goes under "More details":
   - Activity: date, category, title (or weight and unit), trip start/end/distance, session place
     and duration, counter, cost, amount, meter reading, "Charged full", photos. More details:
     notes, tags, trip from/to/duration/battery, remaining level, water "estimated", "meter
     reset" and period dates.
   - Object: type, templates, name, counter, suggested reminders (with the current reading), and
     for a body the weight unit and starting weight. More details: the resource block, "Inside",
     description, tags, purchase date and price, Archive, Private.
   - Reminder: kind, title, "Due by" and the fields it shows. More details: notes.
   - Reading: no optional fields, so no section.
5. **"More details" is a button plus a `hidden` region** (`MoreDetails.svelte`), not `<details>`
   and not bits-ui Collapsible. The fields stay mounted while closed (typed text survives, the
   form still submits them), the toggle is a real `button` with `aria-expanded`, and an
   `invalid` event from a hidden field opens the section synchronously (`flushSync`) so the
   browser can focus it. It also opens when a template or a repeat chip fills one of its fields.
6. **No object type is preselected on a new object.** "Other" as the default filed most new
   objects as Other. Save refuses until a tile is picked, with "Check “Type”: it is missing or
   not valid." under the tiles. A template, a kept draft and the "+ New type…" round trip count as
   a pick. Seven e2e flows that saved a new object without choosing a type now choose one; one
   assertion (`24-own-types`: "a plain visit comes up with Other") changes to "no type chosen".
7. **The unit sits inside the field, not in the label.** `Field` draws "km" at the right edge of
   the control and adds " (km)" to the label for screen readers only. Visible labels lose the
   stacked parentheticals ("Repeat every (counter) (km)" becomes "Then every" with "km" in the
   box); accessible names keep the unit, so `getByLabel(/Reading \(km\)/)`, `/^Start/`,
   `/Battery used/` and the like still match. Labels whose i18n text embeds a single `({unit})`
   (reading, current reading, tank capacity, monthly target) are left as they are.
8. **Errors from the validators go under the field they name.** `fieldErrorAt(key, t, ids)` maps
   the key to a field id; `revealField(id)` opens a closed "More details" around it and focuses
   it. A key with no field on screen, and every server or network error, goes to the line in the
   Save bar. The message text is unchanged, so `16-form-errors` keeps its assertion.
9. **The Save bar is sticky at every width**, not only on phones: the object form is taller than
   an 800 px desktop viewport once its details are open. Below 900 px it sits above the tab bar;
   from 900 px at the bottom of the viewport. It is the form's last child, so at the end of the
   form it never covers a field.
10. **Reminder "Due by" defaults to Date** and is only shown when the object has a counter. A
    saved reminder opens on the side it uses (`dueModeOf`). Switching sides only hides fields;
    the unused side is cleared when saving (`applyDueMode`), so switching back and forth loses
    nothing typed. The server already counts a reminder with both as due at whichever comes
    first, and the hint says so.
11. **A `--ui-warn` token** (`#915700` light, `#f0a640` dark, the values app.css's `--warn`
    already passes with) for warnings in new markup (counter lower than last, weight jump,
    implausible reading). Mapped as `text-warn`, with contrast tests.
12. **The weight unit stays a select** (kg/lb) in both forms: it is a setting of the value, and
    `30-weight` drives it with `selectOption`.
13. **Object page leftovers (spec notes for round 4) are Task 6**: CSV import, Energy/Insights/
    Trips legacy classes, ObjectDetail's `p.error`/`p.muted`, Dashboard's `min-[1024px]:` to
    `wide:`, a contrast test for tag text on the due card, and the double `/reminders` fetch.
    `.fab-row` stays for round 5, which the spec already gives the `.fab` move to.
14. **app.css keeps `.field`, `.row`, `.hint`, `.warn`, `.error` and the element rules**: Settings,
    Login, Setup, Stats and Types still use them (round 5). This round deletes what only the
    forms used: `.chips` and `.thumb-strip`.
15. **The quick-templates key is reused**: `object.quick-templates` becomes the visible label "Or
    start from a template" instead of a new key.

No open question needs the user.

## Global Constraints

- **Chunk boundary.**
  - `$lib/components/ui/**`, `bits-ui`, `@lucide/svelte`, `FormActions`, `MoreDetails`,
    `TypeTiles` and `reveal-field.ts` are imported only by the lazy routes (the four forms,
    `ObjectDetail` and what they import). Nothing `App.svelte` or `Dashboard.svelte` pulls in
    eagerly may import them.
  - The entry chunk (`index-*.js`) must not grow (after ≤ before, gzip -9).
  - Round budget: +10 KB gzip JS in total, measured with `gzip -9` as in rounds 2 and 3.
- WCAG AA: text ≥ 4.5:1; focus rings, control outlines and state indicators ≥ 3:1, in both
  themes. Every new tinted-background/text pair gets a computed-blend test in
  `frontend/tests/theme-contrast.test.ts`. Pairs this round reuses and that are already tested:
  brand-ink on `primary/10` over card (checked tiles and segments), muted-foreground on card
  (unit suffix, placeholder), `input` against card (control outlines).
- Touch targets ≥ 44×44 px on mobile: controls are `h-12`, buttons `min-h-11`, checkboxes widen
  their hit area with `before:-inset-3`.
- Every new string goes into both `frontend/src/i18n/en.ts` and `de.ts` (append at the end of the
  object, before `} as Record<string, string>;`). `tests/i18n.test.ts` checks the key sets match.
- A migrated component deletes its own `<style>` rules for what it restyles. Scoped styles are
  unlayered and beat utilities. All four forms end the round with no `<style>` block.
- app.css's revert block reaches new markup: set `display`, `list-style` (`list-none`, plus
  `role="list"` on `<ul>`), heading sizes and margins (`m-0`) explicitly; set `p-0 border-0` on
  `fieldset`/`legend`. Never use the legacy class names `field`, `row`, `hint`, `warn`, `error`,
  `muted`, `chip`, `chips`, `ghost`, `primary`, `danger` in new markup.
- Plain `<button>`s and native `<input>`/`<select>`/`<textarea>` in new markup carry a
  `data-slot="…"` marker. It exempts them from app.css's legacy element rules.
- Focus rings on every interactive element that is not a shadcn component:
  `focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring`
  (`-outline-offset-2` inside a scroller). Label-drawn controls use the same with `has-focus-visible:`.
  shadcn components keep full-strength rings (the unit test rejects `ring-ring/`).
- Clickable cards use the stretched-link pattern with `isolate` on the card (not needed in forms,
  kept for Task 6's object page). `.grid` is `.thumb-grid` in app.css; do not add a `.grid` class.
- Breakpoints: `desk:` (900 px, the shell) and `wide:` (1024 px, two-column layouts). No script
  breakpoints in forms.
- Bind-and-react: where a control needs to run code after its value changes, use a function
  binding (`bind:value={() => x, (v) => { x = v; after(); }}`), never `bind:value` plus
  `onchange`/`oninput` on a wrapper component: the order of the two listeners is not guaranteed
  once they pass through a component.
- e2e:
  - No test is deleted. A test that asserts removed behaviour changes to assert the new
    behaviour, and every such change is named in the task report.
  - Labels, ids (`#weekday`, `#trip-from`, `#u`, `#pp`) and button names stay as they are unless
    a step names the change.
  - `frontend/tests-e2e/32-calendar-reminders.spec.ts` is being edited in the working tree by
    another agent: locate its lines by content, not by number, and keep their edits.
  - Test ids this round introduces: `form-actions`, `calendar-weekday`, `tag-input`,
    `entry-attachments`, `attachment`, `page-error`.
  - New helpers in `tests-e2e/helpers.ts`: `openMoreDetails(page)` (Task 3),
    `chooseType(page, type)` and `typeTile(page, type)` (Task 4).
- Screenshots before and after each task with `frontend/scripts/shots.mjs`:
  1. Build first: `cd frontend && npm run build && cd .. && cargo build`.
  2. Start the server:
     `rm -rf /tmp/logb-shots && LOGB_DATA_DIR=/tmp/logb-shots LOGB_PORT=8111 LOGB_BIND=127.0.0.1 LOGB_LOG=warn target/debug/logb &`
  3. Capture: `(cd frontend && node scripts/shots.mjs ../shots/r4-tN new,edit)`. After Task 1 the
     filter takes a comma list and captures 08-activity-new, 09-reading-new, 10-reminder-new,
     11-object-new and 19-object-edit. Task 6 uses `object` instead.
  4. Kill the server afterwards (`kill %1`). The `/logb` process under uid 65532 is the user's own
     container; never touch it.
- Commits end with these two trailer lines, after a blank line:
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01NdJo5cgB7BK3PsYZHsHs5T
  ```
- No merge, tag or push: the round stops at a local release commit.

---
### Task 1: The field pattern and the Save bar, on the reading form

**Files:**
- Create: `frontend/src/lib/components/ui/field/classes.ts`, `context.ts`, `field.svelte`,
  `index.ts`; `frontend/src/lib/components/ui/input/input.svelte`, `index.ts`;
  `frontend/src/lib/FormActions.svelte`; `frontend/src/lib/reveal-field.ts`
- Modify: `frontend/src/routes/ReadingForm.svelte`, `frontend/src/lib/form-error.ts`,
  `frontend/src/app.tw.css`, `frontend/src/i18n/en.ts`, `de.ts`, `frontend/scripts/shots.mjs`,
  `frontend/tests/form-error.test.ts`, `frontend/tests/theme-contrast.test.ts`
- Test: `frontend/tests-e2e/38-forms.spec.ts` (new)

**Interfaces:**
- Produces:
  - `import { Field } from '$lib/components/ui/field/index.js'`: props `id`, `label`, `hint?`,
    `warn?`, `error?`, `unit?`, `class?`, `children`.
  - `getFieldContext(): FieldContext | undefined` with `id`, `describedBy`, `invalid`, `unit`.
  - `controlClass`, `labelClass`, `hintClass`, `warnClass`, `errorClass`, `sectionHeadingClass`,
    `chipClass` from `$lib/components/ui/field/classes.js`.
  - `import { Input } from '$lib/components/ui/input/index.js'`.
  - `FormActions.svelte`: props `busy?`, `error?`, `oncancel`; `data-testid="form-actions"`.
  - `fieldErrorAt(key, t, ids): FieldError` and `type FieldError = { id: string | null; message: string }`
    in `form-error.ts`; `revealField(id): Promise<boolean>` in `reveal-field.ts`.
  - Tailwind colour `warn` (`text-warn`).

- [ ] **Step 1: Branch and capture "before"**

Start from the 0.21.0 release commit: from `main` if it has it, otherwise from `ui-round-3`.

```bash
git log main --oneline | grep -q 'release 0.21.0' && git switch -c ui-round-4 main || git switch -c ui-round-4 ui-round-3
cd frontend && npm run build && cd .. && cargo build
mkdir -p .superpowers/sdd
cd frontend && for f in dist/assets/*.js dist/assets/*.css; do printf '%s %s\n' "$(gzip -9c "$f" | wc -c)" "$f"; done > ../.superpowers/sdd/bundle-r4-before.txt; cd ..
rm -rf /tmp/logb-shots && LOGB_DATA_DIR=/tmp/logb-shots LOGB_PORT=8111 LOGB_BIND=127.0.0.1 LOGB_LOG=warn target/debug/logb &
sleep 2 && (cd frontend && node scripts/shots.mjs ../shots/r4-before new); kill %1
```

- [ ] **Step 2: Screens for the forms in `frontend/scripts/shots.mjs`.** Replace
  `const ONLY = process.argv[3];` with

```js
/** Optional comma list of substrings: a screen is captured when its id contains any of them. */
const ONLY = process.argv[3]?.split(',');
```

  replace `if (ONLY && !id.includes(ONLY)) continue;` with
  `if (ONLY && !ONLY.some((s) => id.includes(s))) continue;`, and in `SCREENS` add after the
  `08-activity-new` line and after the `11-object-new` line respectively:

```js
  ['09-reading-new', `/objects/${car}/reading`],
```

```js
  ['19-object-edit', `/objects/${car}/edit`],
```

  Then capture the two new screens for "before" as well:
  run step 1's server lines again with `node scripts/shots.mjs ../shots/r4-before reading,edit`.

- [ ] **Step 3: Failing unit test.** Append to `frontend/tests/form-error.test.ts` (add
  `fieldErrorAt` to its import from `../src/lib/form-error`):

```ts
describe('fieldErrorAt', () => {
  const t = (key: string, vars?: Record<string, string | number>) => (vars ? `${key}(${Object.values(vars).join(',')})` : key);

  it('puts the message on the field the key names', () => {
    expect(fieldErrorAt('activity.cost', t, { 'activity.cost': 'co' })).toEqual({ id: 'co', message: fieldError('activity.cost', t) });
  });

  it('puts a key the form has no field for on the form itself', () => {
    expect(fieldErrorAt('activity.cost', t, {})).toEqual({ id: null, message: fieldError('activity.cost', t) });
  });

  it('keeps the trip sentences', () => {
    expect(fieldErrorAt('trip.end', t, { 'trip.end': 'ten' })).toEqual({ id: 'ten', message: 'trip.error-end' });
  });
});
```

  If the file does not already import `describe`, `it`, `expect` and `fieldError`, add them.

Run: `cd frontend && npx vitest run tests/form-error.test.ts`
Expected: FAIL, `fieldErrorAt` is not exported.

- [ ] **Step 4: `fieldErrorAt`.** Append to `frontend/src/lib/form-error.ts`:

```ts
/** Where a refused field's message goes: the id of the field's element in the form, or `null`
 *  for the form's own message line (a key the form has no field for). */
export interface FieldError { id: string | null; message: string }

/** `fieldError`, plus where to show it. `ids` maps a validator's keys to a form's field ids. */
export function fieldErrorAt(key: string, t: Translate, ids: Readonly<Record<string, string>>): FieldError {
  return { id: ids[key] ?? null, message: fieldError(key, t) };
}
```

Run: `npx vitest run tests/form-error.test.ts` — PASS.

- [ ] **Step 5: The warn token.** In `frontend/src/app.tw.css`:
  - in `:root { … }` after `--ui-brand-ink: #b45309;` add `  /* Warning text (a counter lower than the last one). app.css's --warn, for new markup. */` and `  --ui-warn: #915700;`
  - in `:root[data-theme='dark'] { … }` after `--ui-brand-ink: #fbbf24;` add `  --ui-warn: #f0a640;`
  - in `@theme inline { … }` after `--color-brand-ink: var(--ui-brand-ink);` add `  --color-warn: var(--ui-warn);`

  In `frontend/tests/theme-contrast.test.ts`, in the `'shadcn token contrast'` `text` list, add
  `['warn', 'background'], ['warn', 'card'],` after `['brand-ink', 'background'], ['brand-ink', 'card'],`.

Run: `npx vitest run tests/theme-contrast.test.ts` — PASS.

- [ ] **Step 6: `frontend/src/lib/components/ui/field/classes.ts`**

```ts
/**
 * The look every form control and its text share. In one module so the controls that draw their
 * own label (TagInput, TypeTiles, Segmented) match `Field` exactly.
 */

/** A text control: 48 px tall (the 44 px target plus its border), the `input` token as its
 *  outline (>= 3:1 on page and card), the full-strength ring. `scroll-my-24` keeps a focused
 *  field clear of the sticky top bar and the sticky Save bar. */
export const controlClass =
  'block h-12 w-full min-w-0 scroll-my-24 rounded-lg border border-input bg-card px-3 text-base text-foreground placeholder:text-muted-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring aria-invalid:border-destructive disabled:cursor-not-allowed disabled:opacity-50';
export const labelClass = 'm-0 text-sm font-medium text-foreground';
export const hintClass = 'm-0 text-sm text-muted-foreground';
export const warnClass = 'm-0 text-sm text-warn';
export const errorClass = 'm-0 text-sm font-medium text-destructive';
/** A heading inside a form: well under the page title, the same as the object page's. */
export const sectionHeadingClass = 'm-0 text-xs font-semibold tracking-wide text-muted-foreground uppercase';
/** A one-tap suggestion (repeat an entry, start from a template), in a row that scrolls sideways. */
export const chipClass =
  'inline-flex min-h-11 shrink-0 cursor-pointer items-center whitespace-nowrap rounded-full border border-border bg-card px-3 text-sm text-foreground transition-colors hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring';
```

- [ ] **Step 7: `frontend/src/lib/components/ui/field/context.ts`**

```ts
import { getContext, setContext } from 'svelte';

/** What a control inside `Field` needs from it: its id (the label's `for`), what describes it
 *  (hint, warning, error), whether it is in error, and the unit drawn inside it. Read through
 *  getters, so a control follows an error that comes and goes. */
export interface FieldContext {
  readonly id: string;
  readonly describedBy: string | undefined;
  readonly invalid: boolean;
  readonly unit: string | null;
}

const KEY = Symbol('field');

export function setFieldContext(ctx: FieldContext): void {
  setContext(KEY, ctx);
}

export function getFieldContext(): FieldContext | undefined {
  return getContext<FieldContext | undefined>(KEY);
}
```

- [ ] **Step 8: `frontend/src/lib/components/ui/field/field.svelte`**

```svelte
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '$lib/utils.js';
  import { errorClass, hintClass, labelClass, warnClass } from './classes.js';
  import { setFieldContext } from './context.js';

  /**
   * One form field: label, control, then an optional hint, warning and error, in that order. The
   * control is the child and reads its id, `aria-describedby` and `aria-invalid` from here.
   *
   * `unit` is drawn inside the control at its right edge ("km") instead of in the label, where
   * units used to stack ("Repeat every (counter) (km)"). Screen readers still hear it as part of
   * the label, so the accessible name stays "Due at (km)".
   */
  let { id, label, hint = '', warn = '', error = '', unit = null, class: className, children }: {
    id: string; label: string; hint?: string; warn?: string; error?: string; unit?: string | null;
    class?: string; children: Snippet;
  } = $props();

  const describedBy = $derived(
    [hint && `${id}-hint`, warn && `${id}-warn`, error && `${id}-error`].filter(Boolean).join(' ') || undefined,
  );
  setFieldContext({
    get id() { return id; },
    get describedBy() { return describedBy; },
    get invalid() { return error !== ''; },
    get unit() { return unit; },
  });
</script>

<div data-slot="field" class={cn('flex min-w-0 flex-col gap-1.5', className)}>
  <label for={id} class={labelClass}>{label}{#if unit}<span class="sr-only"> ({unit})</span>{/if}</label>
  {#if unit}
    <div class="relative">
      {@render children()}
      <span aria-hidden="true" class="pointer-events-none absolute inset-y-0 right-3 flex items-center text-sm text-muted-foreground">{unit}</span>
    </div>
  {:else}
    {@render children()}
  {/if}
  {#if hint}<p id={`${id}-hint`} class={hintClass}>{hint}</p>{/if}
  {#if warn}<p id={`${id}-warn`} role="status" class={warnClass}>{warn}</p>{/if}
  {#if error}<p id={`${id}-error`} role="alert" class={errorClass}>{error}</p>{/if}
</div>
```

- [ ] **Step 9: `frontend/src/lib/components/ui/field/index.ts`**

```ts
import Root from './field.svelte';

export { Root, Root as Field };
export { getFieldContext, setFieldContext, type FieldContext } from './context.js';
```

  (Task 4 adds `CheckField` here.)

- [ ] **Step 10: `frontend/src/lib/components/ui/input/input.svelte` and `index.ts`**

```svelte
<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils.js';
  import { controlClass } from '../field/classes.js';
  import { getFieldContext } from '../field/context.js';

  let { ref = $bindable(null), value = $bindable(), type = 'text', id, class: className, ...restProps }:
    HTMLInputAttributes & { ref?: HTMLInputElement | null } = $props();
  const field = getFieldContext();
</script>

<!-- A number field with a unit drawn over its right edge loses the browser's spinner there. -->
<input
  bind:this={ref}
  bind:value
  {type}
  data-slot="input"
  id={id ?? field?.id}
  aria-describedby={field?.describedBy}
  aria-invalid={field?.invalid || undefined}
  class={cn(
    controlClass,
    field?.unit && 'pr-14 [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none',
    className,
  )}
  {...restProps}
/>
```

```ts
import Root from './input.svelte';

export { Root, Root as Input };
```

- [ ] **Step 11: `frontend/src/lib/reveal-field.ts`**

```ts
import { tick } from 'svelte';

/**
 * Brings a refused field into view and focus after a failed save. A field inside a closed "More
 * details" is shown first, through that section's own toggle, so the focus has somewhere to land.
 * Answers false when the form has no such element (the field is not shown for this entry); the
 * caller then puts the message on the form's own line.
 */
export async function revealField(id: string): Promise<boolean> {
  await tick();
  const el = document.getElementById(id);
  if (!el) return false;
  const region = el.closest<HTMLElement>('[hidden]');
  if (region?.id) document.querySelector<HTMLButtonElement>(`[aria-controls="${region.id}"]`)?.click();
  await tick();
  el.focus();
  return true;
}
```

- [ ] **Step 12: `frontend/src/lib/FormActions.svelte`**

```svelte
<script lang="ts">
  import { Button } from '$lib/components/ui/button/index.js';
  import { errorClass } from '$lib/components/ui/field/classes.js';
  import { t } from '../i18n';

  /**
   * Save and Cancel, stuck to the bottom of the screen: above the tab bar on a phone, at the
   * viewport's edge from 900 px. It is the form's last child, so it sticks while the form runs on
   * below the fold and settles under the last field at the end: Save is on screen when the form
   * opens and never covers a field. `error` is the form's own line (a network failure, a key no
   * field shows), kept next to the button that caused it.
   */
  let { busy = false, error = '', oncancel }: { busy?: boolean; error?: string; oncancel: () => void } = $props();
</script>

<div data-testid="form-actions"
     class="sticky z-[6] -mx-3 mt-2 flex flex-col gap-2 border-t border-border bg-background px-3 py-3 max-desk:bottom-[calc(var(--navbar)+1px+env(safe-area-inset-bottom))] desk:bottom-0">
  {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
  <div class="flex gap-2 desk:justify-end">
    <Button variant="outline" class="min-h-11 flex-1 desk:min-w-28 desk:flex-none" onclick={oncancel}>{$t('nav.cancel')}</Button>
    <Button type="submit" class="min-h-11 flex-1 desk:min-w-28 desk:flex-none" disabled={busy}>{$t('nav.save')}</Button>
  </div>
</div>
```

  (`--navbar` is app.css's 56 px; the tab bar adds a 1 px top border.)

- [ ] **Step 13: Failing e2e test.** Create `frontend/tests-e2e/38-forms.spec.ts`:

```ts
import { expect, test, type Page } from '@playwright/test';
import { signInFresh } from './helpers';

/** The forms as round 4 of the UI overhaul left them. Seeds through the API so each test drives
 *  only the form it is about. */
async function object(page: Page, data: Record<string, unknown>): Promise<number> {
  const res = await page.request.post('/api/objects', { data: { description: '', ...data } });
  expect(res.ok()).toBe(true);
  return (await res.json()).id as number;
}

/** Save is on screen: inside the viewport, a full-size target, and above a phone's tab bar. */
async function expectSaveOnScreen(page: Page): Promise<void> {
  const save = page.getByRole('button', { name: 'Save', exact: true });
  await expect(save).toBeVisible();
  const box = (await save.boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(box.y).toBeGreaterThanOrEqual(0);
  expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
  expect(box.height).toBeGreaterThanOrEqual(44);
  const nav = (await page.getByRole('navigation', { name: /Main|Hauptnavigation/ }).boundingBox())!;
  // A bottom tab bar (phone): Save sits above it. A sidebar (desktop) cannot cover it.
  if (nav.y > viewport.height / 2) expect(box.y + box.height).toBeLessThanOrEqual(nav.y + 0.5);
}

test('the reading form: one field pattern, and Save on screen from the start', async ({ page }) => {
  await signInFresh(page, '38-reading');
  const id = await object(page, { name: 'Forms reading car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/reading`);

  const reading = page.getByLabel(/Reading \(km\)/);
  await expect(reading).toBeFocused();
  // The object's name is the page's subtitle now, not a stray paragraph in the form.
  await expect(page.getByText('Forms reading car', { exact: true })).toBeVisible();
  await expectSaveOnScreen(page);

  // The label belongs to its control: clicking it focuses the field.
  await page.getByLabel('Date', { exact: true }).focus();
  await page.getByText('Reading (km)', { exact: true }).click();
  await expect(reading).toBeFocused();
  await expect(reading).toHaveCSS('height', '56px');
});
```

Run: `cd frontend && npm run build && npx playwright test 38-forms`
Expected: FAIL. The name is not on screen as its own text (it is in `p.muted`, which matches, so
the first failure is Save's position on mobile or the 56 px height).

- [ ] **Step 14: Rewrite `frontend/src/routes/ReadingForm.svelte`.** Keep the `<script>` block
  with these changes:
  - add the imports

```ts
  import FormActions from '../lib/FormActions.svelte';
  import { Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { errorClass, hintClass, warnClass } from '$lib/components/ui/field/classes.js';
```

  - after the `warning` derived, add

```ts
  const lastHint = $derived(
    object && object.stats.current_counter !== null && lastDate
      ? $t('reading.last', { counter: counter(object.stats.current_counter, object.counter_unit, $locale), date: fmtDate(lastDate, $dateFormat) })
      : '',
  );
```

  Replace everything from `<main>` to the end of the file (the `<style>` block goes) with:

```svelte
<main>
  <TopBar title={$t('reading.title')} subtitle={object?.name ?? null} backTo={`/objects/${oid}`} />
  {#if object}
    <form onsubmit={submit} class="m-0 flex w-full max-w-[40rem] flex-col gap-5">
      <Field id="rv" label={$t('reading.value', { unit: object.counter_unit ?? '' })} hint={lastHint}>
        <!-- Focused straight away: this form has one job, and it is usually opened from a
             notification with the odometer in front of the person. -->
        <Input type="number" inputmode="numeric" min="0" step="1" bind:value={valueText} autofocus required class="h-14 text-2xl tabular-nums" />
      </Field>
      <Field id="rd" label={$t('activity.date')}><DateInput id="rd" bind:value={date} max={todayIso()} required /></Field>
      <!-- An alert, not the field's quiet warning: it is the reason Save did not go through, and
           saving again unchanged is the confirmation. -->
      {#if acknowledged && warning === acknowledged.warning}
        <p role="alert" class={warnClass}>
          {warning === 'lower'
            ? $t('reading.warn-lower', { last: counter(object.stats.current_counter, object.counter_unit, $locale) })
            : $t('reading.warn-implausible', { date: fmtDate(lastDate, $dateFormat) })}
        </p>
      {/if}
      <FormActions {busy} {error} oncancel={() => back(`/objects/${oid}`)} />
    </form>
  {:else if error}
    <p role="alert" class={errorClass}>{error}</p>
  {:else}
    <p class={hintClass}>{$t('nav.loading')}</p>
  {/if}
</main>
```

- [ ] **Step 15: Verify**

Run: `npm run check && npm test && npx playwright test 38-forms 15-reading 26-date-format`
Expected: PASS, both projects. `15-reading-reminders` still finds the lower-reading warning with
`getByRole('alert')`. Then the full `npm run e2e`: PASS.
Capture `../shots/r4-t1 reading` and compare `09-reading-new` with `shots/r4-before`: the name
under the title, the reading in large figures, the date beneath, Save and Cancel in a bar at the
bottom (above the tab bar on mobile) in both themes.

- [ ] **Step 16: Commit**

```bash
git add -A frontend
git commit -m "feat: field pattern and sticky Save bar, first on the reading form"
```

(plus the two trailer lines.)

### Task 2: The shared inputs: date, tags, files

**Files:**
- Modify: `frontend/src/lib/DateInput.svelte`, `frontend/src/lib/TagInput.svelte`,
  `frontend/src/lib/FilePicker.svelte`, `frontend/tests-e2e/23-tags.spec.ts`,
  `frontend/tests-e2e/32-calendar-reminders.spec.ts`
- Test: `frontend/tests-e2e/38-forms.spec.ts`

**Interfaces:**
- Consumes: `controlClass`, `labelClass`, `errorClass` (Task 1), `getFieldContext` (Task 1).
- Produces: `DateInput` takes `aria-describedby`/`aria-invalid` from a surrounding `Field`; its
  own error keeps the id `${id}-error` and, while shown, is the only `aria-describedby` target
  (`26-date-format` reads the attribute as one id). Test ids `calendar-weekday`, `tag-input`.
  The props of all three components are unchanged.

- [ ] **Step 1: Failing e2e test.** Append to `38-forms.spec.ts`:

```ts
test('the date field keeps its calendar button inside the box, at a full-size target', async ({ page }) => {
  await signInFresh(page, '38-date');
  const id = await object(page, { name: 'Date box car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/reading`);

  const field = (await page.getByLabel('Date', { exact: true }).boundingBox())!;
  const button = page.getByRole('button', { name: 'Choose date', exact: true });
  const b = (await button.boundingBox())!;
  expect(b.x).toBeGreaterThanOrEqual(field.x);
  expect(b.x + b.width).toBeLessThanOrEqual(field.x + field.width);
  expect(b.y).toBeGreaterThanOrEqual(field.y);
  expect(b.y + b.height).toBeLessThanOrEqual(field.y + field.height);
  expect(Math.min(b.width, b.height)).toBeGreaterThanOrEqual(44);

  await button.click();
  await expect(page.getByTestId('calendar-weekday')).toHaveCount(7);
  const day = page.getByRole('dialog', { name: 'Choose date' }).getByRole('button', { pressed: true });
  await expect(day).toHaveCount(1);
  expect((await day.boundingBox())!.height).toBeGreaterThanOrEqual(44);
});
```

Run: `npm run build && npx playwright test 38-forms -g "calendar button"`
Expected: FAIL. The button sits beside the field, outside its box.

- [ ] **Step 2: `DateInput.svelte`.**
  1. Imports: add

```ts
  import { cn } from '$lib/utils.js';
  import { controlClass } from '$lib/components/ui/field/classes.js';
  import { getFieldContext } from '$lib/components/ui/field/context.js';
```

  2. After the `$props()` line add

```ts
  /** The `Field` around this input, if any: its hint and error describe the field too. The
   *  field's own format error, while shown, is what describes it (one id: see 26-date-format). */
  const fieldCtx = getFieldContext();
```

  3. Replace everything from `<div class="date-input"` to the end of the file (markup and
     `<style>` block) with:

```svelte
<div data-slot="date-input" class="relative min-w-0" bind:this={container} onfocusout={(e) => { if (e.relatedTarget instanceof Node && !container.contains(e.relatedTarget)) pickerOpen = false; }}>
  <input {id} bind:this={field} data-slot="date-text" type="text" inputmode="numeric" autocomplete="off" {required} aria-label={label}
         placeholder={datePlaceholder($dateFormat, $locale)} bind:value={text}
         aria-invalid={!!error || fieldCtx?.invalid || undefined}
         aria-describedby={error ? `${id}-error` : fieldCtx?.describedBy}
         class={cn(controlClass, 'pr-12 tabular-nums')}
         onfocus={() => (focused = true)} onblur={() => { focused = false; commit(); }} onchange={commit} />
  <!-- Inside the field's box, at its right edge: beside it, it squeezed the text to "MM/DD/YYY". -->
  <button bind:this={trigger} type="button" data-slot="date-trigger" aria-label={$t('date.pick')} aria-haspopup="dialog" aria-controls={`${id}-calendar`} aria-expanded={pickerOpen} onclick={openPicker}
          class="absolute top-1/2 right-0.5 grid size-11 -translate-y-1/2 cursor-pointer place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring"><Icon name="calendar" size={18} /></button>
  {#if pickerOpen}
    <div id={`${id}-calendar`} role="dialog" aria-label={$t('date.pick')}
         class="absolute top-[calc(100%+4px)] right-0 z-20 w-[min(20rem,90vw)] rounded-lg border border-border bg-popover p-2 text-popover-foreground shadow-md">
      <div class="grid grid-cols-[auto_1fr_auto] items-center text-center">
        <button type="button" data-slot="calendar-nav" class="grid size-11 cursor-pointer place-items-center rounded-md text-lg hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring" aria-label={$t('date.previous-month')} onclick={() => moveMonth(-1)}>‹</button>
        <strong class="text-sm font-semibold">{monthLabel}</strong>
        <button type="button" data-slot="calendar-nav" class="grid size-11 cursor-pointer place-items-center rounded-md text-lg hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring" aria-label={$t('date.next-month')} onclick={() => moveMonth(1)}>›</button>
      </div>
      <div class="grid grid-cols-7 gap-0.5">
        {#each weekdays as day}<span data-testid="calendar-weekday" class="py-1 text-center text-xs text-muted-foreground">{day}</span>{/each}
        {#each calendarDays as day}
          <button type="button" data-slot="calendar-day" data-date={isoDate(day)} tabindex={isoDate(day) === focusedDay ? 0 : -1}
            aria-label={dateTimeFormat($locale, { dateStyle: 'full', timeZone: 'UTC' }).format(day)}
            aria-pressed={isoDate(day) === value} aria-disabled={!!rangeMessage(isoDate(day), $dateFormat)}
            class={[
              'min-h-11 min-w-0 cursor-pointer rounded-md text-sm tabular-nums not-aria-pressed:hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring aria-pressed:bg-primary aria-pressed:font-semibold aria-pressed:text-primary-foreground aria-disabled:cursor-not-allowed aria-disabled:opacity-40',
              day.getUTCMonth() !== view.getUTCMonth() && 'text-muted-foreground',
            ]}
            onkeydown={(e) => calendarKey(e, day)} onclick={() => { if (!rangeMessage(isoDate(day), $dateFormat)) pick(day); }}>{day.getUTCDate()}</button>
        {/each}
      </div>
    </div>
  {/if}
</div>
{#if error}
  <span id={`${id}-error`} class="text-sm font-medium text-destructive" aria-live="polite">{error}</span>
{/if}
```

  (The selected day is the amber fill with its dark text, a tested pair; days of the
  neighbouring months are muted-foreground on popover, also tested.)

- [ ] **Step 3: `TagInput.svelte`.** Add the import
  `import { errorClass, labelClass } from '$lib/components/ui/field/classes.js';` and replace
  everything from `<div class="field">` to the end of the file with:

```svelte
<div data-slot="tag-field" class="flex min-w-0 flex-col gap-1.5">
  <label for={id} class={labelClass}>{label}</label>
  <!-- Looks like one text field: the chips sit inside the box, and the box shows the focus. -->
  <div data-testid="tag-input"
       class="flex min-h-12 flex-wrap items-center gap-1 rounded-lg border border-input bg-card px-2 py-1.5 focus-within:outline-2 focus-within:outline-solid focus-within:outline-offset-2 focus-within:outline-ring">
    {#each tags as tag (tag)}
      <span class={`tag tag-${tagColorIndex(tag)}`}>{tag}
        <!-- A 32 px tall hit area around the bare glyph: it reaches left over the chip's own text
             and right to half the gap, so it never covers the next chip. -->
        <button type="button" data-slot="tag-remove" aria-label={$t('tags.remove', { tag })} onclick={() => (tags = removeTag(tags, tag))}
                class="relative cursor-pointer border-0 bg-transparent p-0 pl-0.5 text-inherit before:absolute before:top-[calc(50%-16px)] before:bottom-[calc(50%-16px)] before:-left-3.5 before:-right-2.5 before:content-[''] focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-1 focus-visible:outline-ring">×</button>
      </span>
    {/each}
    <input {id} data-slot="tag-text" bind:this={field} bind:value={text} {onkeydown} {oninput} onblur={() => text.trim() && add(text)}
           placeholder={$t('tags.placeholder')} autocomplete="off" list={`${id}-list`}
           aria-describedby={error ? `${id}-error` : undefined}
           class="h-9 min-w-32 flex-1 scroll-my-24 border-0 bg-transparent px-1 text-base text-foreground placeholder:text-muted-foreground focus-visible:outline-none" />
    <datalist id={`${id}-list`}>{#each offered as s (s)}<option value={s}></option>{/each}</datalist>
  </div>
  <!-- Always rendered, so the live region exists before the first error is announced. -->
  <p class={errorClass} id={`${id}-error`} aria-live="polite" hidden={!error}>{error}</p>
</div>
```

- [ ] **Step 4: `FilePicker.svelte`.** Add the imports
  `import { Button } from '$lib/components/ui/button/index.js';` and
  `import { errorClass } from '$lib/components/ui/field/classes.js';`, then replace everything from
  `<div class="picker">` to the end of the file with:

```svelte
<div data-slot="file-picker" class="flex flex-col gap-2">
  <input bind:this={el} type="file" class="hidden" multiple accept="image/*,application/pdf,.txt,.md,.doc,.docx,.xls,.xlsx"
         onchange={(e) => send((e.currentTarget as HTMLInputElement).files)} />
  <!-- `capture` cannot live on the input above: on mobile it suppresses picking an existing file. -->
  <input bind:this={cam} type="file" class="hidden" accept="image/*" capture="environment"
         onchange={(e) => send((e.currentTarget as HTMLInputElement).files)} />
  <div class="grid grid-cols-2 gap-2">
    <Button variant="outline" class="min-h-11 border-dashed whitespace-normal" disabled={busy} onclick={() => el.click()}>
      {busy ? $t('activity.uploading') : `+ ${$t('activity.add-files')}`}
    </Button>
    <Button variant="outline" class="min-h-11 border-dashed whitespace-normal" disabled={busy} onclick={() => cam.click()}><Icon name="camera" size={18} /> {$t('activity.take-photo')}</Button>
  </div>
  {#if error}<p class={errorClass} aria-live="polite">{error}</p>{/if}
</div>
```

  The two file inputs stay first and in this order: specs call `page.setInputFiles('input[type=file]', …)`.

- [ ] **Step 5: Move the specs off the removed classes.** Locator changes only:
  - `23-tags.spec.ts` (the edit step of the first-entry test, `page.locator('.tag-input .tag', { hasText: 'BBV' })`)
    becomes `page.getByTestId('tag-input').locator('.tag', { hasText: 'BBV' })`. `.tag` stays: TagChips is round 5's.
  - `32-calendar-reminders.spec.ts`, both `page.locator('.weekday').first()` lines (the English
    "Sun" and the German "Mo" assertions) become `page.getByTestId('calendar-weekday').first()`.

- [ ] **Step 6: Verify, look, commit**

Run: `npm run check && npm test && npx playwright test 38-forms 23-tags 26-date-format 32-calendar 34-photo 16-form 02-lifecycle`
Expected: PASS, both projects. Then the full `npm run e2e`: PASS.
Capture `../shots/r4-t2 new,edit`: date fields show the whole placeholder with the calendar icon
inside; the tag field is one box; the file buttons are dashed outline buttons, readable in dark
mode. Open a calendar by hand once in each theme.

```bash
git add -A frontend
git commit -m "feat: date, tag and file inputs in the new style"
```

### Task 3: The reminder form: "Due by", segmented choices, "More details"

**Files:**
- Create: `frontend/src/lib/components/ui/textarea/textarea.svelte`, `index.ts`;
  `frontend/src/lib/components/ui/native-select/native-select.svelte`, `index.ts`;
  `frontend/src/lib/components/ui/segmented/segmented.svelte`, `index.ts`;
  `frontend/src/lib/MoreDetails.svelte`
- Modify: `frontend/src/routes/ReminderForm.svelte`, `frontend/src/lib/reminder-form.ts`,
  `frontend/src/i18n/en.ts`, `de.ts`, `frontend/tests/reminder-form.test.ts`,
  `frontend/tests-e2e/helpers.ts`, `frontend/tests-e2e/02-lifecycle.spec.ts`
- Test: `frontend/tests-e2e/38-forms.spec.ts`

**Interfaces:**
- Consumes: `Field`, `Input`, `FormActions`, `fieldErrorAt`, `revealField` (Task 1).
- Produces:
  - `Textarea`, `NativeSelect` (both read the `Field` context like `Input`).
  - `Segmented<T extends string>`: props `legend`, `name`, `options: { value: T; label: string }[]`,
    `value` (bindable), `hint?`, `onchange?(value)`, `class?`. Renders a `fieldset` (role group,
    named by the legend) of native radios.
  - `MoreDetails.svelte`: props `open` (bindable), `id?` (default `more-details`), `children`.
    Toggle: `button` named "More details", `aria-expanded`, `aria-controls`.
  - `type DueMode = 'date' | 'counter' | 'both'`, `dueModeOf(input, hasCounter)`,
    `applyDueMode(input, mode)`, `REMINDER_FIELD_IDS` in `reminder-form.ts`.
  - `openMoreDetails(page)` in `tests-e2e/helpers.ts`.
- Changed strings: `reminder.due-counter` "Due at counter" → "Due at" (de "Fällig bei");
  `reminder.repeat-counter` "Repeat every (counter)" → "Then every" (de "Danach alle"). Both are
  shown with the unit inside the field and " (km)" in the accessible name.

- [ ] **Step 1: Failing unit tests.** Append to `frontend/tests/reminder-form.test.ts`, and add
  `applyDueMode, dueModeOf, REMINDER_FIELD_IDS` to its import from `../src/lib/reminder-form`:

```ts
describe('due by date, counter or both', () => {
  const base = emptyReminder();

  it('reads the side a saved reminder uses', () => {
    expect(dueModeOf({ ...base, due_date: '2030-01-01' }, true)).toBe('date');
    expect(dueModeOf({ ...base, due_counter: 100000 }, true)).toBe('counter');
    expect(dueModeOf({ ...base, due_counter: 100000, repeat_counter: 15000 }, true)).toBe('counter');
    expect(dueModeOf({ ...base, due_date: '2030-01-01', due_counter: 100000 }, true)).toBe('both');
    expect(dueModeOf({ ...base, every_n: 12, every_unit: 'month', repeat_counter: 15000 }, true)).toBe('both');
    expect(dueModeOf({ ...base, schedule: 'yearly:3:1', due_counter: 5 }, true)).toBe('both');
  });

  it('is by date for a new reminder, and always by date without a counter', () => {
    expect(dueModeOf(base, true)).toBe('date');
    expect(dueModeOf({ ...base, due_counter: 100000 }, false)).toBe('date');
  });

  it('clears the side the reminder does not use', () => {
    const full = { ...base, due_date: '2030-01-01', every_n: 12, every_unit: 'month' as const, due_counter: 100000, repeat_counter: 15000 };
    expect(applyDueMode(full, 'date')).toEqual({ ...full, due_counter: null, repeat_counter: null });
    expect(applyDueMode(full, 'counter')).toEqual({ ...full, due_date: null, schedule: null, every_n: null, every_unit: null, repeat_months: null });
    expect(applyDueMode(full, 'both')).toEqual(full);
  });

  it('leaves a reading reminder alone', () => {
    const reading = readingReminder('Log it', '2030-01-01');
    expect(applyDueMode(reading, 'counter')).toEqual(reading);
  });

  it('a reminder by counter alone is valid', () => {
    expect(validateReminder(applyDueMode({ ...base, title: 'Belt', due_counter: 100000, due_date: '2030-01-01' }, 'counter'))).toBeNull();
  });

  it('names a field for every key validateReminder returns', () => {
    for (const key of ['reminder.title', 'reminder.every', 'reminder.schedule', 'reminder.due-date', 'reminder.due-counter', 'reminder.repeat-counter']) {
      expect(REMINDER_FIELD_IDS[key], key).toBeTruthy();
    }
  });
});
```

Run: `npx vitest run tests/reminder-form.test.ts`
Expected: FAIL, the three names are not exported.

- [ ] **Step 2: Implement them.** Append to `frontend/src/lib/reminder-form.ts`:

```ts
/** Which of the two due fields a service reminder watches. */
export type DueMode = 'date' | 'counter' | 'both';

/** The side a saved reminder uses, so the form opens on it. Without a counter on the object
 *  there is only the date. A new reminder starts by date. */
export function dueModeOf(input: ReminderInput, hasCounter: boolean): DueMode {
  if (!hasCounter) return 'date';
  const byCounter = input.due_counter !== null || input.repeat_counter !== null;
  const byDate = input.due_date !== null || input.schedule !== null || input.every_n !== null || input.repeat_months !== null;
  if (byCounter && byDate) return 'both';
  return byCounter ? 'counter' : 'date';
}

/** The reminder as saved under `mode`: the side it does not use is cleared, so a value typed and
 *  then hidden by switching sides is not saved behind the user's back. The form applies this only
 *  when saving, so switching back and forth loses nothing typed. Reading reminders have no sides. */
export function applyDueMode(input: ReminderInput, mode: DueMode): ReminderInput {
  if (input.kind === 'reading' || mode === 'both') return input;
  if (mode === 'date') return { ...input, due_counter: null, repeat_counter: null };
  return { ...input, due_date: null, schedule: null, every_n: null, every_unit: null, repeat_months: null };
}

/** The reminder form's field for each key `validateReminder` returns. `reminder.due-date` is the
 *  counter field instead while only the counter side is shown (the form overrides it). */
export const REMINDER_FIELD_IDS: Readonly<Record<string, string>> = {
  'reminder.title': 'ti',
  'reminder.every': 'en',
  'reminder.schedule': 'recurrence',
  'reminder.due-date': 'dd',
  'reminder.due-counter': 'dc',
  'reminder.repeat-counter': 'rc',
};
```

Run: `npx vitest run tests/reminder-form.test.ts` — PASS.

- [ ] **Step 3: `textarea` component.** `frontend/src/lib/components/ui/textarea/textarea.svelte`:

```svelte
<script lang="ts">
  import type { HTMLTextareaAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils.js';
  import { controlClass } from '../field/classes.js';
  import { getFieldContext } from '../field/context.js';

  let { ref = $bindable(null), value = $bindable(), id, class: className, ...restProps }:
    HTMLTextareaAttributes & { ref?: HTMLTextAreaElement | null } = $props();
  const field = getFieldContext();
</script>

<textarea
  bind:this={ref}
  bind:value
  data-slot="textarea"
  id={id ?? field?.id}
  aria-describedby={field?.describedBy}
  aria-invalid={field?.invalid || undefined}
  class={cn(controlClass, 'h-auto min-h-24 resize-y py-2.5', className)}
  {...restProps}
></textarea>
```

`index.ts`:

```ts
import Root from './textarea.svelte';

export { Root, Root as Textarea };
```

- [ ] **Step 4: `native-select` component.** `frontend/src/lib/components/ui/native-select/native-select.svelte`:

```svelte
<script lang="ts">
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import type { HTMLSelectAttributes } from 'svelte/elements';
  import { cn } from '$lib/utils.js';
  import { controlClass } from '../field/classes.js';
  import { getFieldContext } from '../field/context.js';

  let { ref = $bindable(null), value = $bindable(), id, class: className, children, ...restProps }:
    HTMLSelectAttributes & { ref?: HTMLSelectElement | null } = $props();
  const field = getFieldContext();
</script>

<!-- A real <select>, styled: the phone's own picker, typeahead through a long list, and
     `selectOption` in the e2e suite all keep working. Only the box and the chevron are ours. -->
<div data-slot="native-select-wrapper" class={cn('relative min-w-0', className)}>
  <select
    bind:this={ref}
    bind:value
    data-slot="native-select"
    id={id ?? field?.id}
    aria-describedby={field?.describedBy}
    aria-invalid={field?.invalid || undefined}
    class={cn(controlClass, 'cursor-pointer appearance-none pr-10')}
    {...restProps}
  >
    {@render children?.()}
  </select>
  <ChevronDown aria-hidden="true" class="pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2 text-muted-foreground" />
</div>
```

`index.ts`:

```ts
import Root from './native-select.svelte';

export { Root, Root as NativeSelect };
```

- [ ] **Step 5: `segmented` component.** `frontend/src/lib/components/ui/segmented/segmented.svelte`:

```svelte
<script lang="ts" generics="T extends string">
  import { cn } from '$lib/utils.js';
  import { hintClass, labelClass } from '../field/classes.js';

  /**
   * Two or three options side by side. Native radios underneath: the group, one tab stop, the
   * arrow keys, `getByRole('radio')` and `.check()` come from the browser. Each radio is
   * invisible and fills its label, which carries the look; the checked one is amber-tinted with
   * an amber-ink edge (the nav's active pair: >= 4.5:1 text, >= 3:1 edge).
   */
  let { legend, name, options, value = $bindable(), hint = '', onchange, class: className }: {
    legend: string; name: string; options: ReadonlyArray<{ value: T; label: string }>; value: T;
    hint?: string; onchange?: (value: T) => void; class?: string;
  } = $props();
</script>

<fieldset data-slot="segmented" class={cn('m-0 min-w-0 border-0 p-0', className)} aria-describedby={hint ? `${name}-hint` : undefined}>
  <legend class={cn(labelClass, 'mb-1.5 p-0')}>{legend}</legend>
  <div class="grid auto-cols-fr grid-flow-col gap-2">
    {#each options as option (option.value)}
      <label class="relative flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-input bg-card px-2 py-1.5 text-center text-sm font-medium text-balance text-foreground transition-colors hover:not-has-checked:bg-accent has-checked:border-brand-ink has-checked:bg-primary/10 has-checked:font-semibold has-checked:text-brand-ink has-focus-visible:outline-2 has-focus-visible:outline-solid has-focus-visible:outline-offset-2 has-focus-visible:outline-ring">
        <input type="radio" data-slot="segmented-option" {name} value={option.value} checked={value === option.value}
               onchange={() => { value = option.value; onchange?.(option.value); }}
               class="absolute inset-0 m-0 size-full cursor-pointer appearance-none rounded-lg opacity-0" />
        {option.label}
      </label>
    {/each}
  </div>
  {#if hint}<p id={`${name}-hint`} class={cn(hintClass, 'mt-1.5')}>{hint}</p>{/if}
</fieldset>
```

`index.ts`:

```ts
import Root from './segmented.svelte';

export { Root, Root as Segmented };
```

- [ ] **Step 6: `frontend/src/lib/MoreDetails.svelte`**

```svelte
<script lang="ts">
  import { flushSync, type Snippet } from 'svelte';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import { t } from '../i18n';

  /**
   * The optional half of a form. Closed for a new entry; the form sets `open` when what it edits
   * uses any of these fields. Closed means `hidden`, not unmounted: what was typed survives a
   * close, and the form still submits it.
   *
   * A closed section must not swallow a validation error. The browser checks every field on
   * submit, hidden or not, and cannot focus one it cannot show: an invalid date in here would
   * block the save with nothing on screen. So an `invalid` event from inside opens the section
   * at once (`flushSync`), before the browser looks for the field to focus.
   */
  let { open = $bindable(false), id = 'more-details', children }: { open?: boolean; id?: string; children: Snippet } = $props();
</script>

<div data-slot="more-details" class="flex flex-col gap-4 border-t border-border pt-2" oninvalidcapture={() => { if (!open) { open = true; flushSync(); } }}>
  <button type="button" data-slot="more-details-toggle" aria-expanded={open} aria-controls={id}
          class="flex min-h-11 w-full cursor-pointer items-center justify-between gap-2 text-left text-sm font-semibold text-foreground hover:text-brand-ink focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring"
          onclick={() => (open = !open)}>
    {$t('form.more-details')}
    <ChevronDown aria-hidden="true" class={['size-4 text-muted-foreground transition-transform motion-reduce:transition-none', open && 'rotate-180']} />
  </button>
  <div {id} hidden={!open} class="flex flex-col gap-5">{@render children()}</div>
</div>
```

  (Tailwind's preflight hides `[hidden]` with `!important`, so `flex` does not undo it.)

- [ ] **Step 7: i18n.** Append to `en.ts`:

```ts
  'form.more-details': 'More details',
  'reminder.due-by': 'Due by',
  'reminder.due-by-date': 'Date',
  'reminder.due-by-counter': 'Counter',
  'reminder.due-by-both': 'Both',
  'reminder.due-by-both-hint': 'Due at whichever comes first.',
```

  and to `de.ts`:

```ts
  'form.more-details': 'Weitere Angaben',
  'reminder.due-by': 'Fällig nach',
  'reminder.due-by-date': 'Datum',
  'reminder.due-by-counter': 'Zählerstand',
  'reminder.due-by-both': 'Beidem',
  'reminder.due-by-both-hint': 'Fällig bei dem, was zuerst eintritt.',
```

  Change the values: `en.ts` `'reminder.due-counter': 'Due at'`, `'reminder.repeat-counter': 'Then every'`;
  `de.ts` `'reminder.due-counter': 'Fällig bei'`, `'reminder.repeat-counter': 'Danach alle'`.

- [ ] **Step 8: `openMoreDetails` helper.** Append to `frontend/tests-e2e/helpers.ts`:

```ts
/** Opens a form's "More details" section if it is closed. Safe to call when it is open already. */
export async function openMoreDetails(page: Page): Promise<void> {
  const toggle = page.getByRole('button', { name: /^(More details|Weitere Angaben)$/ });
  if ((await toggle.getAttribute('aria-expanded')) !== 'true') await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
}
```

- [ ] **Step 9: Failing e2e tests.** Append to `38-forms.spec.ts` (add `openMoreDetails` to the
  helpers import):

```ts
test('a reminder is due by date, by counter or by both, chosen up front', async ({ page }) => {
  await signInFresh(page, '38-due-by');
  const id = await object(page, { name: 'Due-by car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/reminders/new`);

  const dueBy = page.getByRole('group', { name: 'Due by' });
  await expect(dueBy.getByRole('radio', { name: 'Date', exact: true })).toBeChecked();
  await expect(page.getByLabel('Due date', { exact: true })).toBeVisible();
  await expect(page.getByLabel(/^Due at/)).toHaveCount(0);
  await expectSaveOnScreen(page);

  await dueBy.getByRole('radio', { name: 'Counter', exact: true }).check();
  await expect(page.getByLabel('Due date', { exact: true })).toHaveCount(0);
  await expect(page.getByLabel('Repeat', { exact: true })).toHaveCount(0);
  // Words in the label, the unit in the box: no "(counter) (km)".
  await expect(page.getByText(/\(counter\)/)).toHaveCount(0);
  await page.getByLabel('Title').fill('Timing belt');
  await page.getByLabel(/^Due at/).fill('100000');
  await page.getByLabel(/^Then every/).fill('15000');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${id}\\?tab=reminders$`));

  const rows = (await (await page.request.get(`/api/objects/${id}/reminders`)).json()) as Array<Record<string, unknown>>;
  expect(rows.find((r) => r.title === 'Timing belt')).toMatchObject({ due_date: null, schedule: null, every_n: null, due_counter: 100000, repeat_counter: 15000 });

  // "Both" says what it means, and a saved reminder opens on its own side.
  await page.goto(`/objects/${id}/reminders/new`);
  await dueBy.getByRole('radio', { name: 'Both', exact: true }).check();
  await expect(page.getByText('Due at whichever comes first.')).toBeVisible();
  const belt = rows.find((r) => r.title === 'Timing belt')!;
  await page.goto(`/objects/${id}/reminders/${belt.id}`);
  await expect(dueBy.getByRole('radio', { name: 'Counter', exact: true })).toBeChecked();
});

test('a counter field typed and then hidden is not saved', async ({ page }) => {
  await signInFresh(page, '38-due-switch');
  const id = await object(page, { name: 'Switch car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/reminders/new`);
  const dueBy = page.getByRole('group', { name: 'Due by' });
  await page.getByLabel('Title').fill('Brake fluid');
  await dueBy.getByRole('radio', { name: 'Counter', exact: true }).check();
  await page.getByLabel(/^Due at/).fill('90000');
  await dueBy.getByRole('radio', { name: 'Date', exact: true }).check();
  await page.getByLabel('Due date', { exact: true }).fill('01/01/2031');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${id}\\?tab=reminders$`));
  const rows = (await (await page.request.get(`/api/objects/${id}/reminders`)).json()) as Array<Record<string, unknown>>;
  expect(rows.find((r) => r.title === 'Brake fluid')).toMatchObject({ due_date: '2031-01-01', due_counter: null });
});

test('notes wait under "More details", which opens by itself for a reminder that has them', async ({ page }) => {
  await signInFresh(page, '38-reminder-notes');
  const id = await object(page, { name: 'Notes car', type: 'other' });
  const plain = await (await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Plain', due_date: '2031-01-01' } })).json();
  const noted = await (await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Noted', due_date: '2031-01-01', notes: 'Use DOT 4' } })).json();

  const toggle = page.getByRole('button', { name: 'More details' });
  await page.goto(`/objects/${id}/reminders/${plain.id}`);
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(page.getByLabel('Notes')).toBeHidden();
  await page.goto(`/objects/${id}/reminders/${noted.id}`);
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(page.getByLabel('Notes')).toHaveValue('Use DOT 4');

  // A refused save names the field under it and moves the focus there.
  await page.getByLabel('Title').fill('');
  await page.getByLabel('Title').evaluate((el: HTMLInputElement) => el.removeAttribute('required'));
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page.getByRole('alert')).toHaveText('Check “Title”: it is missing or not valid.');
  await expect(page.getByLabel('Title')).toBeFocused();
  await expect(page.getByLabel('Title')).toHaveAttribute('aria-invalid', 'true');
});
```

  The due-date fill uses the default `mdy` format of a fresh user (`01/01/2031`).

Run: `npm run build && npx playwright test 38-forms -g "reminder|counter field"`
Expected: FAIL. There is no "Due by" group.

- [ ] **Step 10: `ReminderForm.svelte` script.**
  1. Imports: replace `import { fieldError } from '../lib/form-error';` with
     `import { fieldErrorAt, type FieldError } from '../lib/form-error';`, extend the
     reminder-form import to
     `import { applyDueMode, dueModeOf, emptyReminder, readingReminder, reminderBody, REMINDER_FIELD_IDS, toReminderInput, validateReminder, type DueMode } from '../lib/reminder-form';`,
     and add:

```ts
  import FormActions from '../lib/FormActions.svelte';
  import MoreDetails from '../lib/MoreDetails.svelte';
  import { revealField } from '../lib/reveal-field';
  import { Field } from '$lib/components/ui/field/index.js';
  import { hintClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Textarea } from '$lib/components/ui/textarea/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { Segmented } from '$lib/components/ui/segmented/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
```

  2. After `const upcoming = …;` add:

```ts
  /** Which due field a service reminder watches; only asked where the object has a counter. */
  let dueMode = $state<DueMode>('date');
  let moreOpen = $state(false);
  /** A save refused by `validateReminder`, shown under the field it names. */
  let fieldErr = $state<FieldError | null>(null);
  const errorFor = (fid: string): string => (fieldErr?.id === fid ? fieldErr.message : '');
  const formError = $derived(error || (fieldErr?.id === null ? fieldErr.message : ''));
  const hasCounter = $derived(!!object?.counter_unit);
  const showDate = $derived(input.kind === 'reading' || !hasCounter || dueMode !== 'counter');
  const showCounter = $derived(input.kind === 'service' && hasCounter && dueMode !== 'date');
```

  3. In `onMount`, replace
     `if (rid) { input = toReminderInput(await api<Reminder>('GET', `/reminders/${rid}`)); readSchedule(); }`
     with

```ts
    if (rid) {
      input = toReminderInput(await api<Reminder>('GET', `/reminders/${rid}`));
      readSchedule();
      dueMode = dueModeOf(input, !!object.counter_unit);
      moreOpen = input.notes.trim() !== '';
    }
```

  4. Replace the body of `normalized()` with

```ts
    const body = reminderBody({
      ...input,
      due_date: input.due_date || null,
      due_counter: num(input.due_counter),
      repeat_months: num(input.repeat_months),
      repeat_counter: num(input.repeat_counter),
      every_n: num(input.every_n),
    });
    // Without a counter there are no sides; a reminder saved when the object still had one keeps
    // its counter rather than losing it to an edit of its title.
    return hasCounter ? applyDueMode(body, dueMode) : body;
```

  5. Before `async function submit`, add

```ts
  async function reject(key: string) {
    const ids = { ...REMINDER_FIELD_IDS, 'reminder.due-date': showDate ? 'dd' : 'dc' };
    const at = fieldErrorAt(key, $t, ids);
    fieldErr = at;
    if (at.id !== null && !(await revealField(at.id))) fieldErr = { id: null, message: at.message };
  }
```

  6. In `submit`, add `fieldErr = null;` after `e.preventDefault();` and replace
     `if (bad) { error = fieldError(bad, $t); return; }` with `if (bad) { await reject(bad); return; }`.

- [ ] **Step 11: `ReminderForm.svelte` markup.** Replace everything from `<main>` to the end of
  the file (the `<style>` block goes) with:

```svelte
<main>
  <TopBar title={editing ? $t('reminder.edit') : $t('reminder.new')} backTo={`/objects/${oid}?tab=reminders`} />
  <form onsubmit={submit} class="m-0 flex w-full max-w-[40rem] flex-col gap-5">
    <!-- A reading needs a counter to read, and a reminder keeps its kind once saved (the server
         refuses a change), so the choice is only offered where it can be made. -->
    {#if !editing && (object?.counter_unit || object?.type === 'body')}
      <Segmented legend={$t('reminder.kind')} name="kind" value={input.kind} onchange={setKind}
                 options={[
                   { value: 'service', label: $t('reminder.kind-service') },
                   { value: 'reading', label: $t(object?.type === 'body' ? 'weight.log' : 'reminder.kind-reading') },
                 ]} />
    {/if}
    <Field id="ti" label={$t('reminder.title')} error={errorFor('ti')}><Input bind:value={input.title} required /></Field>

    {#if input.kind === 'service' && hasCounter}
      <Segmented legend={$t('reminder.due-by')} name="due-by" bind:value={dueMode}
                 hint={dueMode === 'both' ? $t('reminder.due-by-both-hint') : ''}
                 options={[
                   { value: 'date', label: $t('reminder.due-by-date') },
                   { value: 'counter', label: $t('reminder.due-by-counter') },
                   { value: 'both', label: $t('reminder.due-by-both') },
                 ]} />
    {/if}

    {#if showDate}
      <Field id="recurrence" label={$t('reminder.recurrence')} error={errorFor('recurrence')}>
        <NativeSelect bind:value={() => recurrence, (v) => { recurrence = v; writeSchedule(); }}>
          {#if input.kind === 'service'}<option value="none">{$t('reminder.recurrence-none')}</option>{/if}
          <option value="interval">{$t('reminder.recurrence-interval')}</option>
          <option value="daily">{$t('reminder.recurrence-daily')}</option>
          <option value="weekly">{$t('reminder.recurrence-weekly')}</option>
          <option value="monthly">{$t('reminder.recurrence-monthly')}</option>
          <option value="yearly">{$t('reminder.recurrence-yearly')}</option>
        </NativeSelect>
      </Field>
      {#if recurrence === 'weekly'}
        <Field id="weekday" label={$t('reminder.weekday')}>
          <NativeSelect bind:value={() => weekday, (v) => { weekday = Number(v); writeSchedule(); }}>
            {#each [1, 2, 3, 4, 5, 6, 7] as d (d)}<option value={d}>{$t(`weekday.${d}`)}</option>{/each}
          </NativeSelect>
        </Field>
      {:else if recurrence === 'monthly'}
        <Field id="monthday" label={$t('reminder.month-day')}>
          <NativeSelect bind:value={() => monthDay, (v) => { monthDay = v; writeSchedule(); }}>
            {#each Array.from({ length: 31 }, (_, i) => i + 1) as d (d)}<option value={d}>{d}</option>{/each}
            <option value="last">{$t('reminder.last-day')}</option>
          </NativeSelect>
        </Field>
      {:else if recurrence === 'yearly'}
        <div class="grid grid-cols-2 gap-3">
          <Field id="yearmonth" label={$t('reminder.month')}>
            <NativeSelect bind:value={() => yearMonth, (v) => { yearMonth = Number(v); writeSchedule(); }}>
              {#each Array.from({ length: 12 }, (_, i) => i + 1) as m (m)}<option value={m}>{$t(`month.${m}`)}</option>{/each}
            </NativeSelect>
          </Field>
          <Field id="yearday" label={$t('reminder.month-day')}>
            <NativeSelect bind:value={() => yearDay, (v) => { yearDay = Number(v); writeSchedule(); }}>
              {#each Array.from({ length: 31 }, (_, i) => i + 1) as d (d)}<option value={d}>{d}</option>{/each}
            </NativeSelect>
          </Field>
        </div>
      {/if}
      {#if input.schedule}
        <Field id="schedule-start" label={$t('reminder.starts')} hint={$t('reminder.calendar-hint')}>
          <DateInput id="schedule-start" bind:value={() => input.due_date ?? '', (v) => (input.due_date = v || null)} />
        </Field>
        <p aria-live="polite" class="m-0 text-sm text-muted-foreground tabular-nums">{$t('reminder.preview')}: {upcoming.map((d) => fmtDate(d, $dateFormat)).join(' · ')}</p>
      {/if}
      {#if recurrence === 'interval'}
        <div class="grid grid-cols-2 gap-3">
          <Field id="en" label={$t('reminder.every')} error={errorFor('en')}><Input type="number" min="1" max="60" bind:value={input.every_n} /></Field>
          <Field id="eu" label={$t('reminder.every-unit')}>
            <NativeSelect bind:value={input.every_unit}>
              <option value="week">{$t('reminder.unit-week')}</option>
              <option value="month">{$t('reminder.unit-month')}</option>
            </NativeSelect>
          </Field>
        </div>
      {/if}
      {#if input.kind === 'reading'}
        {#if !input.schedule}
          <Field id="st" label={$t('reminder.starts')}><DateInput id="st" bind:value={() => input.due_date ?? '', (v) => (input.due_date = v || null)} /></Field>
        {/if}
        <p class={hintClass}>{$t(object?.type === 'body' ? 'weight.reminder-hint' : 'reminder.reading-hint')}</p>
      {:else if !input.schedule}
        <Field id="dd" label={$t('reminder.due-date')} error={errorFor('dd')}>
          <DateInput id="dd" bind:value={() => input.due_date ?? '', (v) => (input.due_date = v || null)} />
        </Field>
      {/if}
    {/if}

    {#if showCounter}
      <div class="grid grid-cols-2 gap-3">
        <Field id="dc" label={$t('reminder.due-counter')} unit={object?.counter_unit ?? null} error={errorFor('dc')}>
          <Input type="number" inputmode="numeric" min="0" bind:value={input.due_counter} />
        </Field>
        <Field id="rc" label={$t('reminder.repeat-counter')} unit={object?.counter_unit ?? null} error={errorFor('rc')}>
          <Input type="number" inputmode="numeric" min="1" bind:value={input.repeat_counter} />
        </Field>
      </div>
    {/if}

    <MoreDetails bind:open={moreOpen}>
      <Field id="no" label={$t('reminder.notes')}><Textarea bind:value={input.notes} /></Field>
    </MoreDetails>

    <FormActions {busy} error={formError} oncancel={() => back(`/objects/${oid}?tab=reminders`)} />
  </form>
  {#if editing}
    <section aria-labelledby="reminder-delete" class="mt-8 flex max-w-[40rem] flex-col gap-2 border-t border-border pt-4">
      <h2 id="reminder-delete" class={sectionHeadingClass}>{$t('nav.delete')}</h2>
      <Button variant="destructive" class="min-h-11 w-fit" disabled={busy} onclick={remove}>{$t('nav.delete')}</Button>
    </section>
  {/if}
</main>
```

  Keep `setKind`, `readSchedule`, `writeSchedule`, `num` and `remove` as they are. `#weekday`
  keeps its id (`32-calendar-reminders` selects it by id).

- [ ] **Step 12: `02-lifecycle.spec.ts`.** The car in this test has a km counter, so the new form
  opens by date. Before the `getByLabel(/Due at counter/)` line insert
  `await page.getByRole('group', { name: 'Due by' }).getByRole('radio', { name: 'Counter', exact: true }).check();`,
  and change the two locators:
  - `page.getByLabel(/Due at counter/)` → `page.getByLabel(/^Due at/)`
  - `page.getByLabel(/Repeat every \(counter\)/)` → `page.getByLabel(/^Then every/)`
  Changed behaviour for the report: the counter fields are behind the "Due by: Counter" choice,
  and their labels lost "counter"/"(counter)".

- [ ] **Step 13: Verify, look, commit**

Run: `npm run check && npm test && npx playwright test 38-forms 02-lifecycle 15-reading 18-templates 32-calendar`
Expected: PASS, both projects. Then the full `npm run e2e`: PASS.
Capture `../shots/r4-t3 new,edit` and compare `10-reminder-new`: kind as a two-part control, the
title, "Due by" with Date selected, the repeat select and due date, "More details" closed, Save
bar at the bottom. Check dark mode for the checked segment.

```bash
git add -A frontend
git commit -m "feat: reminder form asks due by date, counter or both; details on request"
```

### Task 4: The object form: type tiles first, templates under a label, details on request

**Files:**
- Create: `frontend/src/lib/components/ui/checkbox/checkbox.svelte`, `index.ts`;
  `frontend/src/lib/components/ui/field/check-field.svelte`; `frontend/src/lib/TypeTiles.svelte`
- Modify: `frontend/src/lib/components/ui/field/index.ts`, `frontend/src/routes/ObjectForm.svelte`,
  `frontend/src/lib/object-form.ts`, `frontend/src/lib/object-draft.ts`, `frontend/src/i18n/en.ts`,
  `de.ts`, `frontend/tests/object-form.test.ts`, `frontend/tests/object-draft.test.ts`,
  `frontend/tests/theme-contrast.test.ts`, `frontend/tests-e2e/helpers.ts`, and the specs listed in
  Step 13
- Test: `frontend/tests-e2e/38-forms.spec.ts`

**Interfaces:**
- Consumes: `Field`, `Input`, `Textarea`, `NativeSelect`, `MoreDetails`, `FormActions`,
  `fieldErrorAt`, `revealField`, `chipClass`, `labelClass`, `hintClass`, `errorClass`,
  `sectionHeadingClass` (Tasks 1–3).
- Produces:
  - `Checkbox` (bits-ui, `data-slot="checkbox"`) and `CheckField` (props `id`, `label`,
    `detail?`, `hint?`, `checked` bindable, `disabled?`), exported from `field/index.ts` and
    `checkbox/index.ts`.
  - `TypeTiles.svelte`: props `value: ObjectType`, `picked: boolean`, `error?`, `onpick(type)`,
    `onnewtype()`. A `fieldset` named "Type"; own types in a nested group "Your types"; the
    "+ New type…" button; error id `object-type-error`; focus target id `object-type`.
  - `objectHasDetails(input)`, `OBJECT_FIELD_IDS` in `object-form.ts`.
  - `saveObjectDraft(path, input, typePicked = true)`, `takeObjectDraftState(path, token)` in
    `object-draft.ts` (`takeObjectDraft` unchanged).
  - `chooseType(page, type)` and `typeTile(page, type)` in `tests-e2e/helpers.ts` (built-in types
    by key, own types by name).
- Changed string: `object.quick-templates` "Quick templates" → "Or start from a template"
  (de "Oder mit einer Vorlage beginnen"); it is now a visible label.

- [ ] **Step 1: Failing unit tests.**
  In `frontend/tests/object-form.test.ts` add `objectHasDetails, OBJECT_FIELD_IDS` to the import
  from `../src/lib/object-form`, `ObjectInput` to the type import, and append:

```ts
describe('object form: More details', () => {
  it('stays closed for a new object', () => {
    expect(objectHasDetails(emptyInput())).toBe(false);
  });

  it('opens for any optional field that is set', () => {
    const cases: Partial<ObjectInput>[] = [
      { resource_kind: 'water' }, { parent_id: 3 }, { description: 'grey' }, { tags: ['Lease'] },
      { purchase_date: '2020-03-01' }, { purchase_price_cents: 100 }, { archived: true }, { private: true },
    ];
    for (const c of cases) expect(objectHasDetails({ ...emptyInput(), ...c }), JSON.stringify(c)).toBe(true);
  });

  it('does not count a description of spaces, or the fields that stay in view', () => {
    expect(objectHasDetails({ ...emptyInput(), description: '   ', name: 'Golf', type: 'car', counter_unit: 'km' })).toBe(false);
  });

  it('names a field for every key validate returns, and for the type', () => {
    for (const key of ['object.type', 'object.name', 'object.purchase-price', 'object.energy-price-error', 'object.fuel-capacity-error']) {
      expect(OBJECT_FIELD_IDS[key], key).toBeTruthy();
    }
  });
});
```

  In `frontend/tests/object-draft.test.ts` add `takeObjectDraftState` to the import and append
  inside `describe('object-draft: save and take', …)`:

```ts
  it('keeps whether a type was chosen', () => {
    const token = saveObjectDraft('/objects/new', input, false);
    expect(takeObjectDraftState('/objects/new', token)).toEqual({ input, typePicked: false });
  });

  it('counts a draft saved without the flag as chosen', () => {
    storage.set('logb.object-draft', JSON.stringify({ path: '/objects/new', token: 't', input }));
    expect(takeObjectDraftState('/objects/new', 't')).toEqual({ input, typePicked: true });
  });
```

Run: `npx vitest run tests/object-form.test.ts tests/object-draft.test.ts`
Expected: FAIL, the names are not exported.

- [ ] **Step 2: `object-form.ts`.** Append:

```ts
/** Whether an object uses anything the form keeps under "More details": the form opens that
 *  section for it, so nothing already filled in sits out of sight. */
export function objectHasDetails(input: ObjectInput): boolean {
  return !!input.resource_kind || input.parent_id != null || input.description.trim() !== ''
    || (input.tags ?? []).length > 0 || input.purchase_date != null || input.purchase_price_cents != null
    || !!input.archived || !!input.private;
}

/** The object form's field for each key `validate` returns, plus the type (checked by the form:
 *  a new object has no type until one is picked). */
export const OBJECT_FIELD_IDS: Readonly<Record<string, string>> = {
  'object.type': 'object-type',
  'object.name': 'n',
  'object.purchase-price': 'pp',
  'object.energy-price-error': 'ep',
  'object.fuel-capacity-error': 'capacity',
};
```

- [ ] **Step 3: `object-draft.ts`.** Replace `saveObjectDraft` and `takeObjectDraft` with:

```ts
export function saveObjectDraft(returnPath: string, input: ObjectInput, typePicked = true): string {
  const token = newOpId();
  try {
    sessionStorage.setItem(KEY, JSON.stringify({ path: returnPath, token, input, typePicked }));
  } catch { /* the shortcut still navigates; the form just comes back empty */ }
  return token;
}

/**
 * Reads back a draft saved for exactly `returnPath` under exactly `token`, and removes whatever
 * was stored either way -- so it is used at most once, and a mount that carries the wrong token
 * (or none at all) both gets nothing back AND discards the stored draft, rather than leaving it
 * sitting there for a later, unrelated visit to pick up. `typePicked` says whether the user had
 * chosen a type before the detour (a new object has none until they do); a draft stored before
 * the flag existed counts as chosen.
 */
export function takeObjectDraftState(returnPath: string, token: string | null): { input: ObjectInput; typePicked: boolean } | null {
  try {
    const raw = sessionStorage.getItem(KEY);
    if (raw === null) return null;
    sessionStorage.removeItem(KEY);
    const parsed = JSON.parse(raw) as { path?: unknown; token?: unknown; input?: unknown; typePicked?: unknown };
    if (token === null || parsed.path !== returnPath || parsed.token !== token) return null;
    return { input: parsed.input as ObjectInput, typePicked: parsed.typePicked !== false };
  } catch {
    return null;
  }
}

/** `takeObjectDraftState`'s input alone. */
export function takeObjectDraft(returnPath: string, token: string | null): ObjectInput | null {
  return takeObjectDraftState(returnPath, token)?.input ?? null;
}
```

Run: `npx vitest run tests/object-form.test.ts tests/object-draft.test.ts` — PASS.

- [ ] **Step 4: The checkbox.** `frontend/src/lib/components/ui/checkbox/checkbox.svelte`:

```svelte
<script lang="ts">
  import { Checkbox as CheckboxPrimitive } from 'bits-ui';
  import CheckIcon from '@lucide/svelte/icons/check';
  import { cn, type WithoutChildrenOrChild } from '$lib/utils.js';

  let { ref = $bindable(null), checked = $bindable(false), class: className, ...restProps }:
    WithoutChildrenOrChild<CheckboxPrimitive.RootProps> = $props();
</script>

<!-- Checked is amber ink with a page-coloured tick, not the amber fill: the box's edge has to
     show against the card (>= 3:1), and amber on white is 2.1:1. The box is 20 px; its hit area
     is widened to 44 px by `before:` where CheckField places it. -->
<CheckboxPrimitive.Root
  bind:ref
  bind:checked
  data-slot="checkbox"
  class={cn(
    'grid size-5 shrink-0 cursor-pointer place-items-center rounded-[5px] border border-input bg-card text-background transition-colors focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:border-brand-ink data-[state=checked]:bg-brand-ink',
    className,
  )}
  {...restProps}
>
  {#snippet children({ checked })}
    {#if checked}<CheckIcon class="size-3.5" strokeWidth={3} aria-hidden="true" />{/if}
  {/snippet}
</CheckboxPrimitive.Root>
```

`index.ts`:

```ts
import Root from './checkbox.svelte';

export { Root, Root as Checkbox };
```

- [ ] **Step 5: `frontend/src/lib/components/ui/field/check-field.svelte`**

```svelte
<script lang="ts">
  import { Checkbox } from '../checkbox/index.js';
  import { hintClass } from './classes.js';

  /** A checkbox with its label beside it and an optional hint under the label. The label toggles
   *  the box; the box's own hit area reaches 44 px. `detail` follows the label, muted
   *  ("Oil change · every 15,000 km"), and is part of its name. */
  let { id, label, detail = '', hint = '', checked = $bindable(false), disabled = false }: {
    id: string; label: string; detail?: string; hint?: string; checked?: boolean; disabled?: boolean;
  } = $props();
</script>

<div data-slot="check-field" class="flex items-start gap-3">
  <span class="flex h-11 shrink-0 items-center">
    <Checkbox {id} bind:checked {disabled} aria-describedby={hint ? `${id}-hint` : undefined}
              class="relative before:absolute before:-inset-3 before:content-['']" />
  </span>
  <span class="flex min-w-0 flex-col">
    <label for={id} class="flex min-h-11 cursor-pointer items-center text-base text-foreground">
      <span>{label}{#if detail}<span class="text-muted-foreground"> · {detail}</span>{/if}</span>
    </label>
    {#if hint}<p id={`${id}-hint`} class={`${hintClass} -mt-2`}>{hint}</p>{/if}
  </span>
</div>
```

  In `field/index.ts` add `import CheckField from './check-field.svelte';` and export it:
  `export { Root, Root as Field, CheckField };`.

- [ ] **Step 6: Contrast.** In `theme-contrast.test.ts`:
  - in the `'shadcn token contrast'` 3:1 list add `['background', 'brand-ink']` (the checkbox's
    tick on its checked fill);
  - in `'tinted highlight contrast'` add:

```ts
    // The destructive button (Delete object/entry/reminder): dark mode paints destructive/20.
    it(`${name}: destructive text on a destructive button (destructive/20 over background) (>= 4.5:1)`, () => {
      const bg = blend(ui('destructive', theme), ui('background', theme), 0.2);
      expect(contrastRatio(ui('destructive', theme), bg)).toBeGreaterThanOrEqual(4.5);
    });
```

Run: `npx vitest run tests/theme-contrast.test.ts` — PASS. If the destructive/20 pair fails in
either theme, report the ratio and use `class="bg-destructive/10"` on the three Delete buttons
instead (the /10 pair is already tested).

- [ ] **Step 7: `frontend/src/lib/TypeTiles.svelte`**

```svelte
<script lang="ts">
  import Icon from './Icon.svelte';
  import { OBJECT_TYPES, type ObjectType } from './types';
  import { customTypes, typeIcon, typesLoaded } from './type-registry';
  import { errorClass, labelClass } from '$lib/components/ui/field/classes.js';
  import { t } from '../i18n';

  /**
   * The object's type as a grid of icon tiles, first on the form: the type decides the counter,
   * the templates and what the object can log. Native radios, invisible, each filling its tile.
   * Nothing is checked until `picked`: a new object has no type until the user chooses one.
   */
  let { value, picked, error = '', onpick, onnewtype }: {
    value: ObjectType; picked: boolean; error?: string; onpick: (type: ObjectType) => void; onnewtype: () => void;
  } = $props();

  /** An own type deleted elsewhere (not synced here yet) still shows as chosen, as "Unknown type". */
  const missing = $derived(value.startsWith('custom:') && !$customTypes.some((c) => c.key === value));
  const tile = 'relative flex min-h-18 cursor-pointer flex-col items-center justify-center gap-1 rounded-lg border border-input bg-card p-2 text-center text-xs font-medium text-foreground transition-colors hover:not-has-checked:bg-accent has-checked:border-brand-ink has-checked:bg-primary/10 has-checked:text-brand-ink has-focus-visible:outline-2 has-focus-visible:outline-solid has-focus-visible:outline-offset-2 has-focus-visible:outline-ring';
</script>

{#snippet option(key: string, label: string)}
  <label class={tile}>
    <input type="radio" data-slot="type-tile" name="object-type" value={key} checked={picked && value === key}
           onchange={() => onpick(key as ObjectType)}
           class="absolute inset-0 m-0 size-full cursor-pointer appearance-none rounded-lg opacity-0" />
    <Icon name={typeIcon(key, $customTypes)} size={22} />
    <span class="line-clamp-2 break-words">{label}</span>
  </label>
{/snippet}

<fieldset data-slot="type-tiles" class="m-0 min-w-0 border-0 p-0" aria-describedby={error ? 'object-type-error' : undefined}>
  <legend class={`${labelClass} mb-1.5 p-0`}>{$t('object.type')}</legend>
  <!-- tabindex -1: where a refused save moves the focus (`OBJECT_FIELD_IDS`). -->
  <div id="object-type" tabindex="-1" class="grid scroll-my-24 grid-cols-3 gap-2 outline-none desk:grid-cols-5">
    {#each OBJECT_TYPES as ty (ty)}{@render option(ty, $t(`type.${ty}`))}{/each}
    {#if missing}{@render option(value, $typesLoaded ? $t('types.unknown') : $t('types.loading'))}{/if}
  </div>
  {#if $customTypes.length > 0}
    <div role="group" aria-labelledby="object-type-yours" class="mt-3 flex flex-col gap-1.5">
      <span id="object-type-yours" class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">{$t('types.yours')}</span>
      <div class="grid grid-cols-3 gap-2 desk:grid-cols-5">
        {#each $customTypes as ct (ct.key)}{@render option(ct.key, ct.name)}{/each}
      </div>
    </div>
  {/if}
  <button type="button" data-slot="new-type" onclick={onnewtype}
          class="mt-1 min-h-11 cursor-pointer text-sm font-medium text-brand-ink underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">{$t('types.new-from-form')}</button>
  {#if error}<p id="object-type-error" role="alert" class={errorClass}>{error}</p>{/if}
</fieldset>
```

- [ ] **Step 8: i18n.** `en.ts`: `"object.quick-templates": "Or start from a template",`.
  `de.ts`: `"object.quick-templates": "Oder mit einer Vorlage beginnen",`.

- [ ] **Step 9: e2e helpers.** Append to `tests-e2e/helpers.ts`:

```ts
const TYPE_LABELS: Record<string, string> = {
  car: 'Car', e_bike: 'E-bike', bike: 'Bicycle', motorcycle: 'Motorcycle', home: 'Home',
  appliance: 'Appliance', tool: 'Tool', body: 'Body', other: 'Other',
};

/** The object form's tile for `type`: a built-in type by key ('car'), an own type by its name. */
export function typeTile(page: Page, type: string) {
  return page.getByRole('group', { name: 'Type', exact: true }).getByRole('radio', { name: TYPE_LABELS[type] ?? type, exact: true });
}

/** Picks the object's type on the object form. A new object has none until one is picked. */
export async function chooseType(page: Page, type: string): Promise<void> {
  await typeTile(page, type).check();
}
```

- [ ] **Step 10: Failing e2e tests.** Append to `38-forms.spec.ts` (add `chooseType`, `typeTile`
  to the helpers import):

```ts
test('a new object starts with the type as tiles, nothing chosen, then templates, then the name', async ({ page }) => {
  await signInFresh(page, '38-type-tiles');
  await page.goto('/objects/new');

  const types = page.getByRole('group', { name: 'Type', exact: true });
  await expect(types.getByRole('radio')).toHaveCount(9);
  await expect(types.getByRole('radio', { checked: true })).toHaveCount(0);
  for (const radio of await types.getByRole('radio').all()) expect((await radio.boundingBox())!.height).toBeGreaterThanOrEqual(44);

  const templates = page.getByRole('region', { name: 'Or start from a template' });
  const typesBox = (await types.boundingBox())!;
  const templatesBox = (await templates.boundingBox())!;
  const nameBox = (await page.getByLabel('Name').boundingBox())!;
  expect(typesBox.y).toBeLessThan(templatesBox.y);
  expect(templatesBox.y).toBeLessThan(nameBox.y);
  await expectSaveOnScreen(page);

  // No type, no save: the message is under the tiles.
  await page.getByLabel('Name').fill('Unfiled thing');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page.getByRole('alert')).toHaveText('Check “Type”: it is missing or not valid.');
  await expect(page).toHaveURL(/\/objects\/new$/);
  await chooseType(page, 'tool');
  await expect(typeTile(page, 'tool')).toBeChecked();
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Unfiled thing' })).toBeVisible();
});

test('object details wait under "More details", which opens for an object that uses them', async ({ page }) => {
  await signInFresh(page, '38-object-details');
  const toggle = page.getByRole('button', { name: 'More details' });
  await page.goto('/objects/new');
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(page.getByLabel('Description')).toBeHidden();

  const plain = await object(page, { name: 'Plain drill', type: 'tool' });
  const described = await object(page, { name: 'Grey car', type: 'car', description: 'grey' });
  await page.goto(`/objects/${plain}/edit`);
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(typeTile(page, 'tool')).toBeChecked();
  await page.goto(`/objects/${described}/edit`);
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(page.getByLabel('Description')).toHaveValue('grey');
  // A real checkbox, full-size target.
  const archive = page.getByRole('checkbox', { name: 'Archive' });
  await expect(archive).not.toBeChecked();
});

test('the Save bar stays on screen while the form scrolls, and never covers the last field', async ({ page }) => {
  await signInFresh(page, '38-sticky');
  await page.goto('/objects/new');
  await openMoreDetails(page);
  await page.evaluate(() => window.scrollTo(0, 400));
  await expectSaveOnScreen(page);
  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  await expectSaveOnScreen(page);
  const bar = (await page.getByTestId('form-actions').boundingBox())!;
  const last = (await page.getByLabel('Private object').boundingBox())!;
  expect(last.y + last.height).toBeLessThanOrEqual(bar.y);
});
```

Run: `npm run build && npx playwright test 38-forms -g "tiles|object details|Save bar"`
Expected: FAIL. `getByRole('group', { name: 'Type' })` finds nothing.

- [ ] **Step 11: `ObjectForm.svelte` script.**
  1. Imports:
     - `import { onMount, untrack } from 'svelte';` stays.
     - Replace `import { fieldError } from '../lib/form-error';` with
       `import { fieldErrorAt, type FieldError } from '../lib/form-error';`.
     - Extend the object-form import with `objectHasDetails, OBJECT_FIELD_IDS`.
     - Replace the types import with
       `import { type ResourceUnit, type MemObject, type ObjectInput, type ObjectType, type TagCount } from '../lib/types';`
       (`OBJECT_TYPES` moves to TypeTiles).
     - Replace the registry import with `import { customTypes, defaultUnit } from '../lib/type-registry';`.
     - Replace the object-draft import with `import { saveObjectDraft, takeObjectDraftState } from '../lib/object-draft';`.
     - Add:

```ts
  import FormActions from '../lib/FormActions.svelte';
  import MoreDetails from '../lib/MoreDetails.svelte';
  import TypeTiles from '../lib/TypeTiles.svelte';
  import { revealField } from '../lib/reveal-field';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { chipClass, errorClass, hintClass, labelClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Textarea } from '$lib/components/ui/textarea/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
```

  2. After the `let input = $state<ObjectInput>(…);` statement add:

```ts
  /** Whether the user has chosen a type. A new object starts with none (the input's own 'other'
   *  is only a placeholder): "Other" as a default filed most new objects as Other. A template, a
   *  kept draft and the "+ New type…" round trip count as a choice; an edit starts chosen. */
  let typePicked = $state(untrack(() => id !== undefined));
  /** "More details": open when the object already uses any of its fields (a `?parent_id=`). */
  let moreOpen = $state(untrack(() => objectHasDetails(input)));
  /** A save refused by `validate` (or for want of a type), shown under the field it names. */
  let fieldErr = $state<FieldError | null>(null);
  const errorFor = (fid: string): string => (fieldErr?.id === fid ? fieldErr.message : '');
```

  3. After `let deleteError = $state('');` add
     `const formError = $derived(error || (fieldErr?.id === null ? fieldErr.message : ''));`.
  4. In `setType`, change `saveObjectDraft(currentPath, $state.snapshot(input))` to
     `saveObjectDraft(currentPath, $state.snapshot(input), typePicked)`. After `setType` add:

```ts
  function pickType(ty: string) {
    typePicked = true;
    if (fieldErr?.id === 'object-type') fieldErr = null;
    setType(ty);
  }
```

  5. At the end of `applyObjectTemplate` add `typePicked = true; if (objectHasDetails(input)) moreOpen = true;`.
  6. Delete the `missingType` derived and its comment (TypeTiles has it).
  7. In `onMount`, replace `const draft = takeObjectDraft(currentPath, draftToken);` with
     `const draft = takeObjectDraftState(currentPath, draftToken);`, and the branch
     `if (draft) {` `fill(draft);` with

```ts
    if (draft) {
      fill(draft.input);
      typePicked = draft.typePicked;
```

     (the `} else if (id) {` branch after it is unchanged). Replace
     `if (typeParam && $customTypes.some((c) => c.key === typeParam)) setType(typeParam);` with
     `if (typeParam && $customTypes.some((c) => c.key === typeParam)) pickType(typeParam);`.
  8. In `fill`, after `input = next;` add `if (objectHasDetails(next)) moreOpen = true;`.
  9. In `applySavedTemplate`, after the `fill(…)` call add `typePicked = true;`.
  10. Before `async function submit` add

```ts
  async function reject(key: string) {
    const at = fieldErrorAt(key, $t, OBJECT_FIELD_IDS);
    fieldErr = at;
    if (at.id !== null && !(await revealField(at.id))) fieldErr = { id: null, message: at.message };
  }
```

  11. In `submit`: add `fieldErr = null;` after `e.preventDefault();`; after the `loadFailed`
      line add `if (!typePicked) { await reject('object.type'); return; }`; replace
      `if (bad) { error = fieldError(bad, $t); return; }` with `if (bad) { await reject(bad); return; }`.

- [ ] **Step 12: `ObjectForm.svelte` markup.** Replace everything from `<main>` to the end of the
  file (the `<style>` block goes) with:

```svelte
<main>
  <TopBar title={editing ? $t('object.edit') : $t('object.new')} backTo={editing ? `/objects/${id}` : '/'} />
  <form onsubmit={submit} class="m-0 flex w-full max-w-[40rem] flex-col gap-5">
    <TypeTiles value={input.type} picked={typePicked} error={errorFor('object-type')} onpick={pickType} onnewtype={() => setType(NEW_TYPE)} />

    {#if !editing}
      <section aria-labelledby="object-templates-label" class="flex flex-col gap-1.5">
        <h2 id="object-templates-label" class={labelClass}>{$t('object.quick-templates')}</h2>
        <div class="-mx-1 flex gap-2 overflow-x-auto px-1 py-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          <button type="button" data-slot="template-chip" class={chipClass} onclick={() => applyObjectTemplate('football')}>{$t('template.object-football')}</button>
          <button type="button" data-slot="template-chip" class={chipClass} onclick={() => applyObjectTemplate('electricity')}>{$t('template.object-electricity')}</button>
          <button type="button" data-slot="template-chip" class={chipClass} onclick={() => applyObjectTemplate('heating-oil')}>{$t('template.object-heating-oil')}</button>
          <button type="button" data-slot="template-chip" class={chipClass} onclick={() => applyObjectTemplate('water')}>{$t('template.object-water')}</button>
          {#each savedTemplates as template (template.id)}
            <span class="inline-flex shrink-0 items-center rounded-full border border-border bg-card">
              <button type="button" data-slot="template-chip" class="min-h-11 cursor-pointer rounded-l-full px-3 text-sm whitespace-nowrap text-foreground hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring"
                      onclick={() => applySavedTemplate(template)}>{template.name}</button>
              <button type="button" data-slot="template-remove" aria-label={$t('template.remove', { name: template.name })}
                      class="grid size-11 cursor-pointer place-items-center rounded-r-full text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring"
                      onclick={() => (savedTemplates = removeObjectTemplate(userId(), template.id))}>×</button>
            </span>
          {/each}
        </div>
      </section>
    {/if}

    <Field id="n" label={$t('object.name')} error={errorFor('n')}><Input bind:value={input.name} required /></Field>

    {#if input.type !== 'body' || editing}
      <Field id="u" label={$t('object.counter')}>
        <NativeSelect bind:value={input.counter_unit}>
          <option value={null}>{$t('object.counter-none')}</option>
          <option value="km">{$t('object.counter-km')}</option>
          <option value="mi">{$t('object.counter-mi')}</option>
          <option value="h">{$t('object.counter-h')}</option>
        </NativeSelect>
      </Field>
      {#if offeredTemplates.length > 0}
        <fieldset class="m-0 flex min-w-0 flex-col gap-1 border-0 p-0" aria-describedby="templates-hint">
          <legend class={`${labelClass} mb-1 p-0`}>{$t('object.templates')}</legend>
          <p id="templates-hint" class={hintClass}>{$t('object.templates-hint')}</p>
          {#each offeredTemplates as tp (tp.id)}
            <CheckField id={`template-${tp.id}`} label={$t(tp.title)} detail={schedule(tp)}
                        bind:checked={() => chosen.includes(tp.id), (on) => (chosen = on ? [...chosen, tp.id] : chosen.filter((c) => c !== tp.id))} />
          {/each}
          {#if needsReading}
            <Field id="cr" label={$t('object.current-reading', { unit: input.counter_unit ?? '' })} hint={$t('object.current-reading-hint')} class="mt-2">
              <Input type="number" inputmode="numeric" min="0" step="1" bind:value={readingText} />
            </Field>
          {/if}
        </fieldset>
      {/if}
    {/if}

    {#if input.type === 'body'}
      <Field id="wu" label={$t('weight.unit')}>
        <NativeSelect bind:value={input.weight_unit}><option value="kg">kg</option><option value="lb">lb</option></NativeSelect>
      </Field>
      {#if !editing}
        <Field id="sw" label={$t('weight.starting')}><Input type="text" inputmode="decimal" bind:value={startingWeight} /></Field>
        {#if startingWeight}
          <Field id="wd" label={$t('activity.date')}><DateInput id="wd" bind:value={weightDate} max={todayIso()} required /></Field>
        {/if}
      {/if}
    {/if}

    <MoreDetails bind:open={moreOpen}>
      {#if input.type !== 'body' || editing}
        <Field id="resource-kind" label={$t('object.resource-kind')}>
          <NativeSelect bind:value={input.resource_kind}>
            <option value={null}>{$t('object.counter-none')}</option>
            <option value="electricity">{$t('resource.electricity')}</option>
            <option value="heating_fuel">{$t('resource.heating-fuel')}</option>
            <option value="vehicle_fuel">{$t('resource.vehicle-fuel')}</option>
            <option value="water">{$t('resource.water')}</option>
          </NativeSelect>
        </Field>
        {#if input.resource_kind}
          <Field id="fu" label={$t('object.resource-unit')}>
            <NativeSelect bind:value={() => input.resource_unit ?? null, setResourceUnit}>
              <option value={null}>{$t('object.counter-none')}</option>
              <option value="l">l</option>
              <option value="gal">gal</option>
              <option value="kwh">{fuelUnitLabel('kwh')}</option>
              <option value="m3">m³</option>
            </NativeSelect>
          </Field>
          {#if input.resource_unit}
            <Field id="ep" label={$t('object.energy-price', { unit: fuelUnitLabel(input.resource_unit) })} error={errorFor('ep')}>
              <Input type="text" inputmode="decimal" bind:value={energyPriceText} />
            </Field>
          {/if}
          {#if input.resource_kind === 'water'}
            <Field id="measurement-mode" label={$t('object.measurement-mode')}>
              <NativeSelect bind:value={input.measurement_mode}>
                <option value="meter">{$t('water.mode-meter')}</option>
                <option value="usage">{$t('water.mode-usage')}</option>
              </NativeSelect>
            </Field>
          {/if}
          <Field id="monthly-target" label={$t('object.monthly-target', { unit: fuelUnitLabel(input.resource_unit ?? null) })}>
            <Input type="text" inputmode="decimal" bind:value={targetText} />
          </Field>
          {#if input.resource_kind === 'heating_fuel' && (input.resource_unit === 'l' || input.resource_unit === 'gal')}
            <div class="grid grid-cols-2 gap-3">
              <Field id="capacity" label={$t('object.fuel-capacity', { unit: fuelUnitLabel(input.resource_unit) })} error={errorFor('capacity')}>
                <Input type="text" inputmode="decimal" bind:value={capacityText} />
              </Field>
              <Field id="low-level" label={$t('object.low-level')}>
                <Input type="number" min="0" max="100" bind:value={input.low_level_pct} />
              </Field>
            </div>
          {/if}
        {/if}
      {/if}
      <Field id="p" label={$t('object.parent')}>
        <NativeSelect bind:value={input.parent_id}>
          <option value={null}>{$t('object.parent-none')}</option>
          {#each parentChoices as p (p.id)}<option value={p.id}>{optionLabel(p)}</option>{/each}
        </NativeSelect>
      </Field>
      <Field id="d" label={$t('object.description')}><Textarea bind:value={input.description} /></Field>
      <TagInput bind:tags={() => input.tags ?? [], (v) => (input.tags = v)} suggestions={tagCounts} label={$t('tags.label')} id="tags" />
      {#if input.type !== 'body' || editing}
        <div class="grid grid-cols-2 gap-3">
          <Field id="pd" label={$t('object.purchase-date')}>
            <DateInput id="pd" bind:value={() => input.purchase_date ?? '', (v) => (input.purchase_date = v || null)} />
          </Field>
          <Field id="pp" label={$t('object.purchase-price')} error={errorFor('pp')}>
            <Input type="text" inputmode="decimal" bind:value={priceText} />
          </Field>
        </div>
      {/if}
      {#if editing}
        <CheckField id="archive" label={$t('object.archive')} hint={$t('object.archived-hint')} bind:checked={input.archived} />
      {/if}
      <CheckField id="private" label={$t('object.private')} hint={$t('object.private-hint')} bind:checked={input.private} />
    </MoreDetails>

    {#if !editing && input.name.trim()}
      <Button variant="outline" class="min-h-11 w-fit" onclick={() => (savedTemplates = saveObjectTemplate(userId(), $state.snapshot(input)))}>{$t('template.save')}</Button>
    {/if}

    <FormActions {busy} error={formError} oncancel={() => back(editing ? `/objects/${id}` : '/')} />
  </form>
  {#if editing}
    <section aria-labelledby="object-delete" class="mt-8 flex max-w-[40rem] flex-col gap-2 border-t border-border pt-4">
      <h2 id="object-delete" class={sectionHeadingClass}>{$t('object.delete')}</h2>
      <p class={hintClass}>{$t('object.delete-hint')}</p>
      <Button variant="destructive" class="min-h-11 w-fit" disabled={busy} onclick={remove}>{$t('object.delete')}</Button>
      {#if deleteError}<p role="alert" class={errorClass}>{deleteError}</p>{/if}
    </section>
  {/if}
</main>
```

  The ids `u` (Counter) and `pp` (Purchase price) are kept: `30-weight` asserts they are absent
  for a new body.

- [ ] **Step 13: Move the specs to the tiles and "More details".** Add `chooseType`, `typeTile`
  and `openMoreDetails` to each file's `./helpers` import as needed.
  1. **Type select → tiles** (locator change; each assertion keeps its meaning). Every
     `await X.getByLabel('Type').selectOption('key');` and
     `await X.getByLabel('Type', { exact: true }).selectOption('key');` becomes
     `await chooseType(X, 'key');` (`X` is `page`, or `a` in `06-outbox-order`). Find them with
     `grep -n "getByLabel('Type'" tests-e2e/*.spec.ts`: `02-lifecycle`, `04-offline` (6),
     `05-pagination` (2), `06-outbox-order` (2), `09-object-types` (6), `11-controls` (4),
     `12-object-hierarchy` (3), `14-settings`, `15-reading-reminders` (2), `18-templates`,
     `23-tags` (4), `26-date-format` (2), `36-dashboard`. `30-weight.spec.ts`:
     `await page.locator('#c').selectOption('body');` → `await chooseType(page, 'body');`.
  2. **`24-own-types.spec.ts`**:
     - `page.locator('#c optgroup[label="Your types"] option', { hasText: 'E-Scooter' })` →
       `page.getByRole('group', { name: 'Your types' }).getByRole('radio', { name: 'E-Scooter', exact: true })` (still `toHaveCount(1)`).
     - `selectOption({ label: 'E-Scooter' })` → `await chooseType(page, 'E-Scooter');`;
       `selectOption({ label: 'Car' })` → `await chooseType(page, 'car');`.
     - Each `…selectOption({ label: '+ New type…' });` (4) →
       `await page.getByRole('button', { name: '+ New type…' }).click();`.
     - `expect(page.getByLabel('Type', { exact: true })).toHaveValue(/^custom:/)` plus the next
       line's `#c option:checked` → `toHaveText('Pedelec')` become
       `await expect(typeTile(page, 'Pedelec')).toBeChecked();` and
       `await expect(typeTile(page, 'Pedelec')).toHaveAttribute('value', /^custom:/);`.
     - `expect(page.getByLabel('Type', { exact: true })).toHaveValue('car')` →
       `await expect(typeTile(page, 'car')).toBeChecked();`.
     - `expect(page.locator('#c option:checked')).toHaveText('Widget')` →
       `await expect(typeTile(page, 'Widget')).toBeChecked();`.
     - **Changed behaviour**, in "abandoning the shortcut leaves a later, plain visit to the
       object form empty": `expect(page.getByLabel('Type', { exact: true })).toHaveValue('other')`
       becomes
       `await expect(page.getByRole('group', { name: 'Type', exact: true }).getByRole('radio', { checked: true })).toHaveCount(0);`
       ("empty" now means no type chosen; Decision 6).
  3. **New objects saved without a type now choose one** (changed behaviour, Decision 6): add
     `await chooseType(page, 'other');` right after the `getByLabel('Name')` fill in
     `12-object-hierarchy` (the "Attic Nest Garage" and "Attic Nest Bulb" flows),
     `16-form-errors` (both tests), `19-offline-edit`, `24-own-types` ("the shortcut also works
     from the edit form": the "Old Name" object) and `34-photo-upload` (its object helper).
     `grep` check: the awk one-liner below prints nothing afterwards.

```bash
awk 'FNR==1{ctx=0} /objects\/new|name: \/New object\//{ctx=FNR; typ=0} ctx && /chooseType|Water meter|Electricity meter|Heating oil|Football/{typ=1} ctx && /name: .Save/{ if(!typ) print FILENAME":"FNR; ctx=0 }' tests-e2e/*.spec.ts
```

  4. **Optional fields**: insert `await openMoreDetails(page);` before the first use of
     `getByLabel('Inside')` in each test of `12-object-hierarchy` (3 places) and before
     `getByLabel('Archive').check()`; in `23-tags` before the first `getByLabel('Tags', { exact: true })`
     of each test that uses it on the **object** form (the first test's "Tag Golf" form, "Enter on
     an empty tags field", "a tag typed past the character limit"). `31-water` needs nothing: the
     Water meter template fills the resource and opens the section.

- [ ] **Step 14: Verify, look, commit**

Run: `npm run check && npm test && npx playwright test 38-forms 02 04 05 06 09 11 12 14 15 16 18 19 23 24 26 30 31 34 36`
Expected: PASS, both projects. Then the full `npm run e2e`: PASS.
Capture `../shots/r4-t4 new,edit` and compare `11-object-new` and `19-object-edit`: the tiles in
3 columns on mobile and 5 on desktop, none selected on new and Car selected on edit; the
templates row labelled; the name below; the suggested-reminder checkboxes after picking Car
with km; "More details" closed on new, open on the Golf's edit (it has a purchase date and tags);
the Delete section below the form. Check the checked tile and the checkbox in dark mode.

```bash
git add -A frontend
git commit -m "feat: object form with type tiles first, labelled templates and details on request"
```

### Task 5: The activity form: one "Notes" label, small headings, details on request

**Files:**
- Modify: `frontend/src/routes/ActivityForm.svelte`, `frontend/src/lib/activity-form.ts`,
  `frontend/src/app.css`, `frontend/tests/activity-form.test.ts`,
  `frontend/tests-e2e/02-lifecycle.spec.ts`, `04-offline.spec.ts`, `23-tags.spec.ts`,
  `28-trips.spec.ts`, `29-charging.spec.ts`
- Test: `frontend/tests-e2e/38-forms.spec.ts`

**Interfaces:**
- Consumes: everything from Tasks 1–4 (`Field`, `CheckField`, `Input`, `Textarea`,
  `NativeSelect`, `MoreDetails`, `FormActions`, `fieldErrorAt`, `revealField`, `chipClass`,
  `sectionHeadingClass`, `openMoreDetails`).
- Produces: `activityHasDetails(input)`, `ACTIVITY_FIELD_IDS` in `activity-form.ts`; test ids
  `entry-attachments` (the list of this entry's files) and `attachment` (one file, with
  `data-pending` while it is only queued).

- [ ] **Step 1: Failing unit tests.** In `frontend/tests/activity-form.test.ts` add
  `activityHasDetails, ACTIVITY_FIELD_IDS` to the import from `../src/lib/activity-form` (and
  `ActivityInput` to its type import if missing), then append:

```ts
describe('activity form: More details', () => {
  it('stays closed for a new entry', () => {
    expect(activityHasDetails(emptyActivity())).toBe(false);
  });

  it('opens for notes, tags, a trip\'s extras, the remaining level and the water flags', () => {
    const cases: Partial<ActivityInput>[] = [
      { notes: 'Torque 120 Nm' }, { tags: ['Winter'] },
      { category: 'trip', from_place: 'Home' }, { category: 'trip', to_place: 'Office' },
      { category: 'trip', duration_minutes: 75 }, { category: 'trip', battery_used_pct: 32 },
      { fuel_level_pct: 40 }, { estimated: 1 }, { meter_reset: 1 },
      { period_start: '2026-01-01' }, { period_end: '2026-02-01' },
    ];
    for (const c of cases) expect(activityHasDetails({ ...emptyActivity(), ...c }), JSON.stringify(c)).toBe(true);
  });

  it('keeps a session\'s place and duration in view', () => {
    expect(activityHasDetails({ ...emptyActivity(), category: 'session', from_place: 'Gym', duration_minutes: 45 })).toBe(false);
  });

  it('names a field for every key validateActivity returns', () => {
    for (const key of ['activity.date', 'activity.title', 'weight.invalid', 'activity.cost', 'activity.counter', 'activity.quantity',
      'activity.fuel-level-error', 'activity.meter-reading', 'activity.period-start', 'trip.start', 'trip.end', 'trip.duration', 'trip.battery']) {
      expect(ACTIVITY_FIELD_IDS[key], key).toBeTruthy();
    }
  });
});
```

Run: `npx vitest run tests/activity-form.test.ts` — FAIL, not exported.

- [ ] **Step 2: Implement.** Append to `frontend/src/lib/activity-form.ts`:

```ts
/** Whether an entry uses anything the form keeps under "More details": the form opens that
 *  section for it. A session's place and duration stay in view (they are what a session is),
 *  so `from_place`/`duration_minutes` only count for a trip. */
export function activityHasDetails(a: ActivityInput): boolean {
  const trip = a.category === 'trip'
    && (!!a.from_place || !!a.to_place || a.duration_minutes != null || a.battery_used_pct != null);
  return a.notes.trim() !== '' || (a.tags ?? []).length > 0 || trip || a.fuel_level_pct != null
    || a.estimated === 1 || a.meter_reset === 1 || !!a.period_start || !!a.period_end;
}

/** The activity form's field for each key `validateActivity` returns. */
export const ACTIVITY_FIELD_IDS: Readonly<Record<string, string>> = {
  'activity.date': 'd',
  'activity.title': 'ti',
  'weight.invalid': 'weight',
  'activity.cost': 'co',
  'activity.counter': 'cv',
  'activity.quantity': 'qt',
  'activity.fuel-level-error': 'fuel-level',
  'activity.meter-reading': 'meter-reading',
  'activity.period-start': 'period-start',
  'trip.start': 'tst',
  'trip.end': 'ten',
  'trip.duration': 'tdu',
  'trip.battery': 'tba',
};
```

Run: `npx vitest run tests/activity-form.test.ts` — PASS.

- [ ] **Step 3: Failing e2e test.** Append to `38-forms.spec.ts`:

```ts
test('the activity form: one Notes label, headings under the title, details on request', async ({ page }) => {
  await signInFresh(page, '38-activity');
  const id = await object(page, { name: 'Activity form car', type: 'car', counter_unit: 'km' });
  await page.goto(`/objects/${id}/activities/new`);

  const size = async (loc: ReturnType<Page['locator']>) => loc.evaluate((el) => parseFloat(getComputedStyle(el).fontSize));
  const title = await size(page.getByRole('heading', { level: 1 }));
  expect(await size(page.getByRole('heading', { name: 'Photos & documents' }))).toBeLessThan(title);
  await expect(page.getByRole('heading', { name: 'Notes' })).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'Details' })).toHaveCount(0);
  await expectSaveOnScreen(page);

  // The unit sits in the field; the label is words.
  await expect(page.getByLabel(/^Counter reading \(km\)$/)).toBeVisible();

  await expect(page.getByLabel('Notes')).toBeHidden();
  await openMoreDetails(page);
  await expect(page.getByText('Notes', { exact: true })).toHaveCount(1);
  await page.getByLabel('Title').fill('Wheel bolts');
  await page.getByLabel('Notes').fill('Torque 120 Nm');
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/objects/${id}$`));

  // Opening it again: its notes are there, so the section is open.
  await page.getByTestId('timeline-entry').filter({ hasText: 'Wheel bolts' }).getByRole('button', { name: 'Wheel bolts' }).click();
  await expect(page.getByRole('button', { name: 'More details' })).toHaveAttribute('aria-expanded', 'true');
  await expect(page.getByLabel('Notes')).toHaveValue('Torque 120 Nm');
});

test('a refused field inside a closed "More details" is shown and focused', async ({ page }) => {
  await signInFresh(page, '38-activity-reveal');
  const id = await object(page, { name: 'Reveal bike', type: 'e_bike', counter_unit: 'km' });
  await page.goto(`/objects/${id}/activities/new?category=trip`);
  await page.getByLabel(/^Start/).fill('100');
  await page.getByLabel(/^End/).fill('150');
  await openMoreDetails(page);
  await page.getByLabel(/Battery used/).fill('101');
  await page.getByRole('button', { name: 'More details' }).click();
  await page.getByRole('button', { name: 'Save', exact: true }).click();
  await expect(page.getByRole('button', { name: 'More details' })).toHaveAttribute('aria-expanded', 'true');
  await expect(page.getByLabel(/Battery used/)).toBeFocused();
  await expect(page.getByRole('alert')).toHaveText('Battery used must be 0–100 %');
});
```

Run: `npm run build && npx playwright test 38-forms -g "activity form|refused field"`
Expected: FAIL. The "Notes" heading still exists.

- [ ] **Step 4: `ActivityForm.svelte` script.**
  1. Imports: replace `import { fieldError } from '../lib/form-error';` with
     `import { fieldError, fieldErrorAt, type FieldError } from '../lib/form-error';`; add
     `activityHasDetails, ACTIVITY_FIELD_IDS` to the `../lib/activity-form` import; add:

```ts
  import FormActions from '../lib/FormActions.svelte';
  import MoreDetails from '../lib/MoreDetails.svelte';
  import { revealField } from '../lib/reveal-field';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { chipClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Textarea } from '$lib/components/ui/textarea/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
```

  2. After `let busy = $state(false);` add:

```ts
  /** A save refused by `validateActivity`, shown under the field it names. */
  let fieldErr = $state<FieldError | null>(null);
  const errorFor = (fid: string): string => (fieldErr?.id === fid ? fieldErr.message : '');
  /** The Save bar's line: anything that is not about one field. */
  const formError = $derived(error || (fieldErr?.id === null ? fieldErr.message : ''));
  /** "More details": opened for an entry that already uses any of its fields. */
  let moreOpen = $state(false);
```

  3. After the `photoDate` derived add:

```ts
  const previousWeight = $derived(object?.stats.latest_weight_grams != null
    ? `${$t('weight.previous')}: ${formatWeight(object.stats.latest_weight_grams, weightUnit, $locale)} · ${fmtDate(object.stats.latest_weight_date ?? null, $dateFormat)}`
    : '');
```

  4. In `onMount`'s edit branch, after `attachments = a.attachments;` add `moreOpen = activityHasDetails(input);`.
  5. At the end of `repeat()` add `if (s.last_from_place !== null || s.last_to_place !== null) moreOpen = true;`.
  6. Before `async function submit` add:

```ts
  async function reject(key: string) {
    const at = fieldErrorAt(key, $t, ACTIVITY_FIELD_IDS);
    fieldErr = at;
    if (at.id !== null && !(await revealField(at.id))) fieldErr = { id: null, message: at.message };
  }

  /** The first "+ Add photos or files": saves the draft the files will hang on (`ensureSaved`). */
  async function addFiles() {
    try { await ensureSaved(); error = ''; } catch (e) { error = errorMessage(e, $t); }
  }
```

  7. In `submit`: add `fieldErr = null;` after `e.preventDefault();`, and replace
     `if (bad) { error = fieldError(bad, $t); return; }` with `if (bad) { await reject(bad); return; }`.
     (`fieldError` stays imported: `ensureSaved` uses it.)

- [ ] **Step 5: `ActivityForm.svelte` markup.** Replace everything from `<main>` to the end of
  the file (the `<style>` block goes) with:

```svelte
<main>
  <TopBar title={editing ? $t('activity.edit') : input.category === 'weight' ? $t('weight.log') : $t('activity.new')} backTo={`/objects/${oid}`} />
  <form onsubmit={submit} class="m-0 flex w-full max-w-[40rem] flex-col gap-5">
    <div class="grid grid-cols-2 gap-3">
      <Field id="d" label={$t('activity.date')} error={errorFor('d')}>
        <DateInput id="d" bind:value={input.date} max={input.category === 'weight' ? todayIso() : undefined} required />
      </Field>
      <Field id="c" label={$t('activity.category')}>
        <NativeSelect bind:value={() => input.category, (v: Category) => { input.category = v; categoryTouched = true; }}>
          {#each offered as c (c)}<option value={c}>{$t(`cat.${c}`)}</option>{/each}
        </NativeSelect>
      </Field>
    </div>
    {#if photoDate && photoDate !== input.date}
      <button type="button" data-slot="photo-date" onclick={() => (input.date = photoDate)}
              class="-mt-3 min-h-11 w-fit cursor-pointer text-left text-sm font-medium text-brand-ink underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">
        {$t('activity.use-exif-date', { date: fmtDate(photoDate, $dateFormat) })}
      </button>
    {/if}
    {#if input.category !== 'weight' && !editing && suggestions.length > 0}
      <div class="-mx-1 -mt-2 flex gap-2 overflow-x-auto px-1 py-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {#each suggestions.slice(0, 3) as s (s.title + s.category)}
          <button type="button" data-slot="repeat-chip" class={chipClass} onclick={() => repeat(s)}>{$t('activity.repeat')}: {s.title}</button>
        {/each}
      </div>
    {/if}

    {#if input.category === 'weight'}
      <div class="grid grid-cols-[1fr_7rem] gap-3">
        <Field id="weight" label={$t('cat.weight')} hint={previousWeight} warn={weightWarn ? $t('weight.large-change') : ''} error={errorFor('weight')}>
          <Input type="text" inputmode="decimal" bind:value={weightText} required />
        </Field>
        <Field id="weight-unit" label={$t('weight.unit')}>
          <NativeSelect bind:value={() => weightUnit, changeWeightUnit}><option value="kg">kg</option><option value="lb">lb</option></NativeSelect>
        </Field>
      </div>
    {:else}
      <!-- Optional only for a trip, a charge or a usage: the fallback word is the placeholder,
           not pre-filled text to notice and delete. `activityTitle` is what the timeline shows
           for an untitled row, so the two never disagree. -->
      <Field id="ti" label={$t('activity.title')} error={errorFor('ti')}>
        <Input list="titles" bind:value={input.title} required={input.category !== 'trip' && input.category !== 'fuel' && input.category !== 'usage'}
               placeholder={activityTitle('', input.category, $t, object?.fuel_unit ?? undefined) || undefined} />
        <datalist id="titles">{#each suggestions as s (s.title + s.category)}<option value={s.title}></option>{/each}</datalist>
      </Field>
    {/if}

    {#if input.category === 'usage' && object?.resource_kind === 'water' && object.measurement_mode === 'meter'}
      <Field id="meter-reading" label={$t('water.meter-reading')} unit={fuelUnitLabel(resourceUnit)} error={errorFor('meter-reading')}>
        <Input type="text" inputmode="decimal" bind:value={meterReadingText} required />
      </Field>
    {/if}
    {#if input.category === 'fuel' || (input.category === 'usage' && object?.resource_kind !== 'water')}
      <!-- "Charged full" for a kWh object, "Filled up" for a tank, picked like every other
           charge/fill string (energyLabelKey). -->
      <CheckField id="charged-full" label={$t(energyLabelKey(resourceUnit) === 'energy.charged' ? 'activity.charged-full' : 'activity.filled-full')} bind:checked={chargedFull} />
    {/if}

    {#if input.category === 'trip'}
      <!-- `object?.counter_unit`: an existing trip must stay editable offline even when the
           object could not be loaded; the fields then show no unit. -->
      <div class="grid grid-cols-2 gap-3">
        <Field id="tst" label={$t('trip.start')} unit={object?.counter_unit ?? null} error={errorFor('tst')}>
          <Input type="number" inputmode="numeric" min="0" bind:value={() => input.start_counter, (v) => { input.start_counter = v; onTripStartChange(); }} />
        </Field>
        <Field id="ten" label={$t('trip.end')} unit={object?.counter_unit ?? null} error={errorFor('ten')}>
          <Input type="number" inputmode="numeric" min="0" bind:value={() => input.counter_value, (v) => { input.counter_value = v; onTripEndChange(); }} />
        </Field>
      </div>
      <!-- No `min` on Distance: an end below start makes it negative, which is exactly what
           `trip.error-end` explains; a native bound would block the submit before it could. -->
      <Field id="tds" label={$t('trip.distance')} unit={object?.counter_unit ?? null}>
        <Input type="number" inputmode="numeric" bind:value={() => distance, (v) => { distance = v; onTripDistanceChange(); }} />
      </Field>
    {/if}
    {#if input.category === 'session'}
      <div class="grid grid-cols-2 gap-3">
        <Field id="session-location" label={$t('session.location')}><Input maxlength={80} bind:value={fromText} /></Field>
        <Field id="session-duration" label={$t('session.duration')}><Input type="text" inputmode="numeric" placeholder="h:mm" bind:value={durationText} /></Field>
      </div>
    {/if}

    {#if input.category !== 'weight'}
      {@const withCounter = !!object?.counter_unit && input.category !== 'trip'}
      <div class="grid grid-cols-2 gap-3">
        {#if withCounter && object?.counter_unit}
          <Field id="cv" label={$t('activity.counter')} unit={object.counter_unit} error={errorFor('cv')}
                 warn={counterWarn ? $t('activity.counter-warn', { last: fmtCounter(lastCounter, object.counter_unit, $locale) }) : ''}>
            <Input type="number" inputmode="numeric" min="0" bind:value={counterText} />
          </Field>
        {/if}
        <Field id="co" label={$t('activity.cost')} error={errorFor('co')} class={withCounter ? '' : 'col-span-2'}>
          <Input type="text" inputmode="decimal" bind:value={costText} />
        </Field>
      </div>
    {/if}
    {#if (input.category === 'fuel' || (input.category === 'usage' && object?.measurement_mode !== 'meter')) && resourceUnit}
      <Field id="qt" label={$t('activity.quantity')} unit={fuelUnitLabel(resourceUnit)} error={errorFor('qt')}>
        <Input type="text" inputmode="decimal" bind:value={quantityText} />
      </Field>
    {/if}

    <section aria-labelledby="activity-photos" class="flex flex-col gap-3">
      <h2 id="activity-photos" class={sectionHeadingClass}>{$t('activity.photos')}</h2>
      {#if attachments.length > 0}
        <ul data-testid="entry-attachments" role="list" class="m-0 flex list-none gap-2 overflow-x-auto p-0">
          {#each attachments as a (a.id)}
            <!-- The item is the image's own size: a positioning context for the "pending" label. -->
            <li data-testid="attachment" data-pending={a.pending ? '' : undefined} class="relative shrink-0 data-pending:opacity-60">
              {#if a.kind === 'photo'}
                <img class="block size-16 rounded-md object-cover" src={a.pending ? a.previewUrl : fileUrl(a.file_id, true)} alt="" loading="lazy" decoding="async" />
              {:else}
                <span class="grid size-16 place-items-center rounded-md bg-muted text-muted-foreground"><Icon name="document" size={28} /></span>
              {/if}
              {#if a.pending}<span class="absolute inset-x-0.5 bottom-0.5 rounded-sm bg-muted px-0.5 text-center text-xs leading-tight text-muted-foreground">{$t('timeline.pending')}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      {#if saved}
        <FilePicker objectId={oid} activityId={saved.id} onuploaded={(a) => (attachments = [...attachments, a])} />
      {:else if ready}
        <Button variant="outline" class="min-h-11 w-full border-dashed" onclick={addFiles}>+ {$t('activity.add-files')}</Button>
      {/if}
    </section>

    <MoreDetails bind:open={moreOpen}>
      <Field id="no" label={$t('activity.notes')}><Textarea bind:value={input.notes} /></Field>
      <TagInput bind:tags={() => input.tags ?? [], (v) => (input.tags = v)} suggestions={tagCounts} label={$t('tags.label')} id="tags" />
      {#if input.category === 'trip'}
        <div class="grid grid-cols-2 gap-3">
          <Field id="tfr" label={$t('trip.from')}>
            <Input list="trip-from" maxlength={80} bind:value={fromText} />
            <datalist id="trip-from">{#each tripPlaces.from as p (p)}<option value={p}></option>{/each}</datalist>
          </Field>
          <Field id="tto" label={$t('trip.to')}>
            <Input list="trip-to" maxlength={80} bind:value={toText} />
            <datalist id="trip-to">{#each tripPlaces.to as p (p)}<option value={p}></option>{/each}</datalist>
          </Field>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <Field id="tdu" label={$t('trip.duration')} error={errorFor('tdu')}>
            <Input type="text" inputmode="numeric" placeholder="h:mm" bind:value={durationText} />
          </Field>
          <!-- No `min`/`max`: 101 must reach `trip.error-battery` instead of a silent refusal. -->
          <Field id="tba" label={$t('trip.battery')} unit="%" error={errorFor('tba')}>
            <Input type="number" inputmode="numeric" bind:value={input.battery_used_pct} />
          </Field>
        </div>
      {/if}
      {#if (input.category === 'fuel' || (input.category === 'usage' && object?.resource_kind !== 'water')) && (resourceUnit === 'l' || resourceUnit === 'gal')}
        <Field id="fuel-level" label={$t('activity.fuel-level')} hint={$t('activity.fuel-level-hint')} error={errorFor('fuel-level')}>
          <Input type="number" inputmode="numeric" min="0" max="100" bind:value={input.fuel_level_pct} />
        </Field>
      {/if}
      {#if input.category === 'usage' && object?.resource_kind === 'water'}
        {#if object.measurement_mode === 'meter'}
          <CheckField id="meter-reset" label={$t('water.meter-reset')} bind:checked={() => input.meter_reset === 1, (v) => (input.meter_reset = v ? 1 : 0)} />
        {:else}
          <div class="grid grid-cols-2 gap-3">
            <Field id="period-start" label={$t('water.period-start')} error={errorFor('period-start')}>
              <DateInput id="period-start" bind:value={() => input.period_start ?? '', (v) => (input.period_start = v || null)} />
            </Field>
            <Field id="period-end" label={$t('water.period-end')}>
              <DateInput id="period-end" bind:value={() => input.period_end ?? '', (v) => (input.period_end = v || null)} />
            </Field>
          </div>
        {/if}
        <CheckField id="estimated" label={$t('water.estimated')} bind:checked={() => input.estimated === 1, (v) => (input.estimated = v ? 1 : 0)} />
      {/if}
    </MoreDetails>

    <FormActions {busy} error={formError} oncancel={cancel} />
  </form>
  {#if editing}
    <section aria-labelledby="activity-delete" class="mt-8 flex max-w-[40rem] flex-col gap-2 border-t border-border pt-4">
      <h2 id="activity-delete" class={sectionHeadingClass}>{$t('nav.delete')}</h2>
      <Button variant="destructive" class="min-h-11 w-fit" onclick={remove}>{$t('nav.delete')}</Button>
    </section>
  {/if}
</main>
```

  Notes for the implementer:
  - `Category` is already imported as a type.
  - The weight unit column is fixed (`7rem`) so the hint and the warning wrap under the weight.
  - The water-usage (not meter) case keeps its quantity field in view (the `qt` block): that is
    the usage amount. Only its period dates move under "More details".

- [ ] **Step 6: app.css.** Nothing uses these any more (`grep -rn 'class="chips\|thumb-strip' src`
  prints nothing): delete the whole `.chips { … }` rule with the comment block above it,
  `.chips::-webkit-scrollbar { … }`, `.chips:focus-within { … }` with its comment,
  `.chips button:where(:not([data-slot])) { … }`, and the two `.thumb-strip` rules.

- [ ] **Step 7: Move the specs.**
  - `02-lifecycle.spec.ts`: `page.locator('.thumb-strip img')` (2) →
    `page.getByTestId('entry-attachments').locator('img')`; `page.locator('.thumb-strip > *').first()`
    → `page.getByTestId('attachment').first()`. Locator changes only.
  - `04-offline.spec.ts`: `page.locator('.thumb-strip .strip-item.pending')` →
    `page.getByTestId('attachment').and(page.locator('[data-pending]'))`. Locator change.
  - `23-tags.spec.ts`: `page.locator('.thumb-strip .strip-item')` → `page.getByTestId('attachment')`.
    Insert `await openMoreDetails(page);` before the first `getByLabel('Tags', { exact: true })`
    of each test that uses it on the **activity** form (the first test's entry and its edit
    step, the offline test, the draft test, "a tag still being typed…", "a typed tag that cannot
    be added…"). The edit step opens by itself (the entry has a tag); the helper is a no-op there.
  - `28-trips.spec.ts`: insert `await openMoreDetails(page);` before the first
    `getByLabel(/^From/)` fill, and before the `getByLabel(/Battery used/).fill('101')` step
    (a fresh trip form, closed again).
  - `29-charging.spec.ts`: insert `await openMoreDetails(page);` before each
    `getByLabel(/Battery used/)` fill (2).
  Changed behaviour for the report: trip From/To/Duration/Battery and the tags are behind "More
  details" on a new entry.

- [ ] **Step 8: Verify, look, commit**

Run: `npm run check && npm test && npx playwright test 38-forms 02 04 09 16 23 24 26 28 29 30 31 34`
Expected: PASS, both projects. Then the full `npm run e2e`: PASS.
Capture `../shots/r4-t5 new,edit` and compare `08-activity-new`: date and category side by
side, the repeat chip, the title, counter (with "km" inside) and cost, the small "PHOTOS &
DOCUMENTS" heading with the dashed button, "More details" closed, the Save bar. Open More
details by hand once: one "Notes" label, tags below.

```bash
git add -A frontend
git commit -m "feat: activity form with one Notes label, small headings and details on request"
```

### Task 6: The object page's leftovers from round 3

The spec's notes for round 4 list what round 3 left on the object page. `.fab-row` stays for
round 5 (Decision 13).

**Files:**
- Modify: `frontend/src/lib/ResourceCsvImport.svelte`, `frontend/src/lib/EnergyFigures.svelte`,
  `frontend/src/lib/Insights.svelte`, `frontend/src/lib/TripTotals.svelte`,
  `frontend/src/lib/Reminders.svelte`, `frontend/src/routes/ObjectDetail.svelte`,
  `frontend/src/routes/Dashboard.svelte`, `frontend/tests/theme-contrast.test.ts`,
  `frontend/tests-e2e/37-object-detail.spec.ts`
- Test: `frontend/tests-e2e/38-forms.spec.ts` (the reminders fetch), `37-object-detail.spec.ts`

**Interfaces:**
- Consumes: `CheckField` (Task 4), `Button`.
- Produces: `Reminders` takes `onloaded?: (rows: Reminder[]) => void`, called with the server's
  rows after each successful load. `ObjectDetail`'s page error carries `data-testid="page-error"`.

- [ ] **Step 1: Failing e2e test.** Append to `38-forms.spec.ts`:

```ts
test('the Reminders tab asks the server for the reminders once', async ({ page }) => {
  await signInFresh(page, '38-reminders-once');
  const id = await object(page, { name: 'Once car', type: 'car', counter_unit: 'km' });
  expect((await page.request.post(`/api/objects/${id}/reminders`, { data: { title: 'Once overdue', due_date: '2000-01-01' } })).ok()).toBe(true);

  const gets: string[] = [];
  page.on('request', (r) => { if (r.method() === 'GET' && new URL(r.url()).pathname === `/api/objects/${id}/reminders`) gets.push(r.url()); });
  await page.goto(`/objects/${id}?tab=reminders`);
  await expect(page.getByTestId('reminder-card')).toHaveCount(1);
  // The summary's due reminder comes from the same answer.
  await expect(page.getByTestId('summary-due-reminder')).toContainText('Once overdue');
  await page.waitForLoadState('networkidle');
  expect(gets).toHaveLength(1);
});
```

Run: `npm run build && npx playwright test 38-forms -g "once"`
Expected: FAIL, `gets` has 2 entries.

- [ ] **Step 2: One fetch.**
  - `Reminders.svelte`: extend the props to
    `let { objectId, unit, activities, onchanged, onloaded, body = false }:` with the type
    `{ objectId: number; body?: boolean; unit: CounterUnit; activities: Activity[]; onchanged?: () => void; onloaded?: (rows: Reminder[]) => void }`,
    and in `load()` after `items = [...queued, ...rows];` add `onloaded?.(rows);`.
  - `ObjectDetail.svelte`: replace
    `$effect(() => { oid; if (dueCount > 0) loadDue(); else { dueSeq.invalidate(); dueReminders = []; } });`
    with

```ts
  // On the Reminders tab the tab's own list answers for the summary too (`onloaded` below): one
  // GET, not two. `untrack`: leaving the tab must not ask again for what it just loaded.
  $effect(() => {
    oid;
    if (dueCount === 0) { dueSeq.invalidate(); dueReminders = []; }
    else if (untrack(() => shownTab) !== 'reminders') loadDue();
  });
```

    and change the `<Reminders …>` element to

```svelte
              <Reminders body={object.type === 'body'} objectId={oid} unit={object.counter_unit} {activities}
                         onloaded={(rows) => { dueSeq.invalidate(); dueReminders = rows.filter((r) => r.due && r.done_at === null); }}
                         onchanged={() => { loadObject(); loadActivities('refresh'); refreshInsights(); refreshDetails(); }} />
```

    (The tab reloads itself before `onchanged`, so the summary is already current.)

- [ ] **Step 3: Page error and loading line in `ObjectDetail.svelte`.**
  `{#if error}<p class="error">{error}</p>{/if}` →
  `{#if error}<p data-testid="page-error" class="m-0 mb-3 text-sm font-medium text-destructive">{error}</p>{/if}`;
  the `<p class="muted">{$t('nav.loading')}</p>` → `<p class="m-0 text-sm text-muted-foreground">{$t('nav.loading')}</p>`.
  In `37-object-detail.spec.ts`, `page.locator('main > p.error')` → `page.getByTestId('page-error')`
  (locator change).

- [ ] **Step 4: `ResourceCsvImport.svelte`.** Add
  `import { Button } from '$lib/components/ui/button/index.js';` and replace everything from
  `<section>` to the end with:

```svelte
<section class="flex flex-col gap-2">
  <h3 class="m-0 mt-4 text-sm font-semibold text-foreground">{$t('resource.csv-title')}</h3>
  <p class="m-0 text-sm text-muted-foreground">{$t('resource.csv-hint')}</p>
  <!-- The browser's own file control, its button drawn like an outline button. -->
  <input data-slot="csv-file" aria-label={$t('resource.csv-file')} type="file" accept=".csv,text/csv" onchange={selected}
         class="block w-full text-sm text-muted-foreground file:mr-3 file:min-h-11 file:cursor-pointer file:rounded-lg file:border file:border-input file:bg-card file:px-3 file:text-sm file:font-medium file:text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring" />
  {#if count > 0}<Button variant="outline" class="min-h-11 w-fit" disabled={busy} onclick={run}>{$t('resource.csv-import', { n: count })}</Button>{/if}
  {#if error}<p class="m-0 text-sm font-medium text-destructive" role="alert">{error}</p>{/if}
</section>
```

- [ ] **Step 5: `EnergyFigures.svelte`.** Each `<p class="muted">` and
  `<p class="muted" data-testid="energy-battery">` gets `class="m-0 text-sm text-muted-foreground"`
  instead (keep the test id); each `<b>` inside becomes `<b class="font-semibold text-foreground tabular-nums">`.

- [ ] **Step 6: `Insights.svelte`.**
  - Add `import { CheckField } from '$lib/components/ui/field/index.js';` and replace the
    `<label class="row toggle">…</label>` block with

```svelte
  <CheckField id="include-contents" label={$t('insights.contents')} bind:checked={() => contents, (on) => oncontents?.(on)} />
```

  - `{#if error}<p class="error" role="alert">{error}</p>{/if}` →
    `{#if error}<p class="m-0 text-sm font-medium text-destructive" role="alert">{error}</p>{/if}`.
  - `<p data-testid="insights-ownership">` → `<p data-testid="insights-ownership" class="m-0">`;
    `<b class="tnum">` → `<b class="font-semibold tabular-nums">`;
    `<span class="muted">` → `<span class="text-sm text-muted-foreground">`.
  - Every `<p class="muted">` → `<p class="m-0 text-sm text-muted-foreground">`, and the plain
    `<b>` inside those paragraphs → `<b class="font-semibold text-foreground tabular-nums">`.
  - `grep -n 'class="\(muted\|tnum\|error\|row\)' src/lib/Insights.svelte` prints nothing.
  `21-object-cost-depth` keeps `getByLabel('Include contents')` with `.check()`/`toBeChecked()`:
  the bits-ui checkbox is `role="checkbox"` with `aria-checked`.

- [ ] **Step 7: `TripTotals.svelte`.** Delete the `<style>` block and set the classes in markup:
  - `<div class="table-wrap" …>` → `<div class="overflow-x-auto focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring" …>` (keep `data-testid`, `tabindex`, `aria-label`);
  - `<table class="tnum">` → `<table class="w-full border-collapse text-sm tabular-nums">`;
  - the empty header `<th></th>` → `<th class="p-1"></th>`; each `<th scope="col">` gets
    `class="p-1 text-right font-semibold whitespace-nowrap"`;
  - each `<th scope="row">` gets `class="p-1 text-left font-normal text-muted-foreground"` (row
    labels wrap, so the three period columns fit at 390 px);
  - each `<td>` gets `class="p-1 text-right whitespace-nowrap"`.
  Keep the component's comments; move the one explaining the wrapping row labels above the
  `<tbody>`.

- [ ] **Step 8: `Dashboard.svelte`.** In the object list,
  `min-[1024px]:grid-cols-2` → `wide:grid-cols-2` (the same 1024 px; `min-[1440px]:grid-cols-3`
  stays, there is no named breakpoint for it).

- [ ] **Step 9: Contrast test for tags on the due card.** In `theme-contrast.test.ts`, inside
  `'tinted highlight contrast'`, add:

```ts
    // Tag chips on a due reminder card (Reminders tab). The chip is opaque, so its own pair holds;
    // its text must also read on the card's tint, where the chip's rounded edge meets it.
    it(`${name}: tag text on its chip and on the due card's tint (destructive/10 over background) (>= 4.5:1)`, () => {
      const legacy = name === 'light' ? light : dark;
      const card = blend(ui('destructive', theme), ui('background', theme), 0.1);
      for (let n = 0; n < 8; n++) {
        expect(contrastRatio(token(`tag-${n}-fg`, legacy), token(`tag-${n}-bg`, legacy)), `tag-${n} chip`).toBeGreaterThanOrEqual(4.5);
        expect(contrastRatio(token(`tag-${n}-fg`, legacy), card), `tag-${n} on card`).toBeGreaterThanOrEqual(4.5);
      }
    });
```

Run: `npx vitest run tests/theme-contrast.test.ts` — PASS. A failing hue is reported with its
ratio, not fixed by changing the tag palette in this task.

- [ ] **Step 10: Verify, look, commit**

Run: `npm run check && npm test && npx playwright test 38-forms 37-object 21-object 27-last 28-trips 29-charging 31-water 36-dashboard`
Expected: PASS, both projects. Then the full `npm run e2e`: PASS.
Capture `../shots/r4-t6 object` and compare `07-object-info` with `shots/r3-t6b`: the Energy,
Insights and Trips text in the new muted style, the "Include contents" checkbox (seed has the
house with the boiler inside), the Trips table unchanged in layout.

```bash
git add -A frontend
git commit -m "fix: object page leftovers: CSV import, figures text, one reminders fetch, wide: on the dashboard"
```

- [ ] **Extra step (added by the controller after round 3): two follow-ups from the flaky-test investigation.**
  1. `frontend/src/App.svelte`: when a route chunk fails to load, the app reloads the page once (`logb.chunk-reload`) even when the browser is offline. Offline, that reload lands on the browser's own error page and strands any queued write. Only reload when `navigator.onLine` is true; offline, keep the "Loading…" state and retry the chunk import on the `online` event. Add a unit or e2e test: start offline with an uncached route chunk, navigate, assert the page is still the app (not `chrome-error://`), then go online and assert the route renders.
  2. `frontend/tests-e2e/32-calendar-reminders.spec.ts`, "counter and weight reminders": it types the title before the form has loaded, and the late load overwrites it. Wait for the form to be ready (e.g. the Save button enabled, or the loaded form field value) before typing.

### Task 7: Budget check and release 0.22.0

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `frontend/package.json`, `frontend/package-lock.json`,
  `docs/openapi.json`, `docs/upgrading.md`

- [ ] **Step 1: Measure**

```bash
cd frontend && npm run build && for f in dist/assets/*.js dist/assets/*.css; do printf '%s %s\n' "$(gzip -9c "$f" | wc -c)" "$f"; done > ../.superpowers/sdd/bundle-r4-after.txt; cd ..
for f in before after; do awk -v n=$f '/\.js$/ {s+=$1} END {print n, "js", s}' .superpowers/sdd/bundle-r4-$f.txt; done
grep 'index-' .superpowers/sdd/bundle-r4-before.txt .superpowers/sdd/bundle-r4-after.txt
grep -l "bits-ui\|floating-ui\|lucide" frontend/dist/assets/index-*.js
grep -rln "components/ui\|FormActions\|MoreDetails\|TypeTiles\|reveal-field" frontend/src/App.svelte frontend/src/routes/Dashboard.svelte frontend/src/lib/AppNav.svelte frontend/src/lib/TopBar.svelte
```

Expected:
- The JS total grows by ≤ 10240 bytes.
- The `index-*.js` entry chunk does not grow (the `wide:` rename shrinks it by a few bytes).
- Both `grep -l`/`grep -rln` print nothing.

If a limit is exceeded, report the numbers and the biggest contributors instead of bumping.
The likely ones are the bits-ui checkbox, the chunk bits-ui's core moves into now that the
forms share it, and the four rewritten forms. Compare the form chunks before and after.

- [ ] **Step 2: Bump to 0.22.0**

```bash
sed -i 's/^version = "0.21.0"/version = "0.22.0"/' Cargo.toml
cargo build   # updates the logb entry in Cargo.lock
(cd frontend && npm version 0.22.0 --no-git-tag-version)
sed -i '0,/"version": "0.21.0"/s//"version": "0.22.0"/' docs/openapi.json
git diff --stat   # exactly the six files, Cargo.lock only the logb version
```

  Add above `## 0.21.0` in `docs/upgrading.md`:

```markdown
## 0.22.0: new look, the forms

**Every form has the same shape**: a label, the field, a hint where one helps, and an error
under the field it is about, with the cursor moved there. Units sit inside their field ("km"),
not in the label. Save and Cancel stay at the bottom of the screen (above the tab bar on a
phone), so Save is visible as soon as a form opens.

**Optional fields wait under "More details"**, which opens by itself when you edit an entry
that uses any of them: notes, tags, a trip's places, duration and battery, an object's resource
settings, "Inside", description, purchase details, Archive and Private.

**New object**: the type comes first, as a grid of icon tiles, and none is chosen for you;
templates follow under "Or start from a template", then the name.

**New reminder**: "Due by: Date / Counter / Both" says which fields the reminder watches. A
reminder due by both is due at whichever comes first. Switching sides keeps what you typed until
you save; only the side you chose is saved.

**Log activity**: one "Notes" field, smaller section headings.

Checkboxes and choices are restyled; drop-down lists stay the phone's own pickers.

No migration.
```

- [ ] **Step 3: Verify and commit**

```bash
cargo clippy --all-targets -- -D warnings
cargo test --test it openapi::
(cd frontend && npm run check && npm test && npm run e2e)
git add Cargo.toml Cargo.lock frontend/package.json frontend/package-lock.json docs/openapi.json docs/upgrading.md
git commit -m "chore: release 0.22.0"
```

(plus the two trailer lines.)

Report:
- the bundle deltas: JS total, entry chunk, and the form chunks;
- the unit and e2e test counts;
- every changed e2e assertion:
  - `24-own-types` "a plain visit…": no type chosen instead of "Other";
  - the seven flows that now choose a type before saving a new object (`12-object-hierarchy` ×2,
    `16-form-errors` ×2, `19-offline-edit`, `24-own-types` "edit form", `34-photo-upload`);
  - `02-lifecycle`: the reminder picks "Due by: Counter", and its labels are "Due at" and
    "Then every";
  - the tests that now open "More details" (`12`, `23`, `28`, `29`);
  - the locator-only moves (`02`, `04`, `23`, `32`, `37`, and every Type select to `chooseType`);
- before/after screenshots: `shots/r4-before` against `shots/r4-t5` for the forms and
  `shots/r3-t6b` against `shots/r4-t6` for the object page.

Stop there: merge, tag and push are the user's decision.
