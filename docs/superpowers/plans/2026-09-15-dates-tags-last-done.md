# Date Format, Tags on Entries, Last Done, Own Types Shortcut Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A chosen date format everywhere (with an own date field), tags visible wherever entries and reminders are listed, a "last done" overview per repeated title, and a shortcut to create an own type from the object form.

**Architecture:** Pure formatting/parsing helpers in `frontend/src/lib/format.ts` driven by a per-device setting; one `DateInput.svelte` replaces native date inputs. One new read endpoint (`last-done`) and one new list filter (`title`) on the Rust side; the rest is Svelte wiring.

**Tech Stack:** Rust (axum, sqlx `Any` on SQLite + PostgreSQL), Svelte 5 runes, TypeScript, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-15-dates-tags-last-done-design.md`

## Global Constraints

- Branch `build-dates-tags-last-done`; never commit to `main`. Before every commit `git branch --show-current` must print `build-dates-tags-last-done`. Stage only files the task changes.
- Portable SQL (SQLite and PostgreSQL); `CAST(... AS BIGINT)` for aggregates; PostgreSQL runs use a throwaway `postgres:17` container on port 55446 that is always stopped afterwards. Tests pick PostgreSQL via `LOGB_TEST_DATABASE_URL`.
- Every UI string in en and de. German Intl output asserted by pattern.
- Date format ids exactly: `auto`, `dmy-dot` (15.09.2026), `dmy-slash` (15/09/2026), `mdy-slash` (09/15/2026), `iso` (2026-09-15). Setting field `dateFormat` in the `logb.settings` store, default `auto`.
- Stored dates stay `YYYY-MM-DD`; nothing on the server changes for dates.
- Playwright with project defaults (never pass `--workers`); tests that rely on `page.route` for cached API paths use `test.use({ serviceWorkers: 'block' })`.
- Run commands in the foreground (Bash timeout up to 600000 ms, split long runs); never end a turn while a command runs or to wait for a notification. Never weaken an existing assertion. Comments explain *why*.
- Commits end with exactly:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01DNUZLftSTtND7ABvN6eoGv
  ```

---

### Task 1: Date format setting, formatter and date field

**Files:**
- Modify: `frontend/src/lib/format.ts` (replace `fmtDate`; add `parseDate`, `resolveDateFormat`, `datePlaceholder`, `DATE_FORMATS`)
- Modify: `frontend/src/stores/settings.ts` (add `dateFormat`)
- Create: `frontend/src/stores/date-format.ts` (derived resolved store)
- Create: `frontend/src/lib/DateInput.svelte`
- Modify: `frontend/src/routes/settings/Appearance.svelte` (new select)
- Modify every `fmtDate` caller: `frontend/src/lib/Reminders.svelte`, `frontend/src/lib/Timeline.svelte`, `frontend/src/routes/ActivityForm.svelte`, `frontend/src/routes/Dashboard.svelte`, `frontend/src/routes/ObjectDetail.svelte`, `frontend/src/routes/ReadingForm.svelte`, `frontend/src/routes/Search.svelte`, `frontend/src/routes/settings/ApiAccess.svelte`, `frontend/src/routes/settings/Database.svelte`
- Replace `<input type="date">` in: `frontend/src/routes/ActivityForm.svelte:304`, `frontend/src/routes/ReadingForm.svelte:84`, `frontend/src/routes/ReminderForm.svelte:97` and `:101`, `frontend/src/routes/ObjectForm.svelte:213`
- Modify: `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: `frontend/tests/format.test.ts`, create `frontend/tests-e2e/26-date-format.spec.ts`; update existing e2e specs that fill native date inputs (find with `grep -rn "type=\"date\"\|fill('20\|selectOption.*date\|getByLabel('Date')" frontend/tests-e2e`)

**Interfaces:**
- Produces: `export type DateFormat = 'dmy-dot' | 'dmy-slash' | 'mdy-slash' | 'iso'`; `export type DateFormatPref = 'auto' | DateFormat`; `fmtDate(iso: string | null | undefined, format: DateFormat): string`; `parseDate(text: string, format: DateFormat): string | null` (returns `YYYY-MM-DD`, `''` → `null` handled by caller); `resolveDateFormat(pref: DateFormatPref, locale: 'en' | 'de', languages: readonly string[]): DateFormat`; store `dateFormat: Readable<DateFormat>` exported from `frontend/src/stores/date-format.ts`; component `DateInput` with props `{ id: string; value: string (bindable, 'YYYY-MM-DD' or ''); min?: string; max?: string; required?: boolean; label?: string }`.

- [ ] **Step 1: Failing unit tests** in `frontend/tests/format.test.ts` (replace the existing `fmtDate` cases that pass a locale; keep every other test):

```ts
import { fmtDate, parseDate, resolveDateFormat, datePlaceholder } from '../src/lib/format';

describe('fmtDate', () => {
  it('renders each format with zero-padded day and month', () => {
    expect(fmtDate('2026-09-05', 'dmy-dot')).toBe('05.09.2026');
    expect(fmtDate('2026-09-05', 'dmy-slash')).toBe('05/09/2026');
    expect(fmtDate('2026-09-05', 'mdy-slash')).toBe('09/05/2026');
    expect(fmtDate('2026-09-05', 'iso')).toBe('2026-09-05');
  });
  it('takes the date part of a timestamp and ignores empty input', () => {
    expect(fmtDate('2026-09-05T23:30:00Z', 'dmy-dot')).toBe('05.09.2026');
    expect(fmtDate(null, 'dmy-dot')).toBe('');
    expect(fmtDate('', 'iso')).toBe('');
  });
});

describe('parseDate', () => {
  it('reads the chosen pattern and lenient variants', () => {
    expect(parseDate('15.09.2026', 'dmy-dot')).toBe('2026-09-15');
    expect(parseDate('1.5.26', 'dmy-dot')).toBe('2026-05-01');
    expect(parseDate('1/5/2026', 'dmy-slash')).toBe('2026-05-01');
    expect(parseDate('5-1-2026', 'mdy-slash')).toBe('2026-05-01');
    expect(parseDate('2026-05-01', 'iso')).toBe('2026-05-01');
    expect(parseDate(' 15.09.2026 ', 'dmy-dot')).toBe('2026-09-15');
  });
  it('rejects impossible or malformed dates', () => {
    expect(parseDate('31.02.2026', 'dmy-dot')).toBeNull();
    expect(parseDate('13/13/2026', 'mdy-slash')).toBeNull();
    expect(parseDate('15.09', 'dmy-dot')).toBeNull();
    expect(parseDate('abc', 'iso')).toBeNull();
    expect(parseDate('', 'dmy-dot')).toBeNull();
  });
});

describe('resolveDateFormat', () => {
  it('keeps an explicit choice', () => {
    expect(resolveDateFormat('iso', 'de', ['de-DE'])).toBe('iso');
  });
  it('auto: German means dd.mm.yyyy, English follows the region', () => {
    expect(resolveDateFormat('auto', 'de', ['en-US'])).toBe('dmy-dot');
    expect(resolveDateFormat('auto', 'en', ['en-US'])).toBe('mdy-slash');
    expect(resolveDateFormat('auto', 'en', ['en'])).toBe('mdy-slash');
    expect(resolveDateFormat('auto', 'en', ['en-GB', 'en-US'])).toBe('dmy-slash');
    expect(resolveDateFormat('auto', 'en', ['de-DE', 'en-AU'])).toBe('dmy-slash');
  });
});

describe('datePlaceholder', () => {
  it('spells the pattern in the app language', () => {
    expect(datePlaceholder('dmy-dot', 'de')).toBe('TT.MM.JJJJ');
    expect(datePlaceholder('dmy-dot', 'en')).toBe('DD.MM.YYYY');
    expect(datePlaceholder('mdy-slash', 'en')).toBe('MM/DD/YYYY');
    expect(datePlaceholder('iso', 'de')).toBe('JJJJ-MM-TT');
  });
});
```

`auto` for English picks the first tag in `languages` whose primary subtag is `en` and has a region; `en-US` or no such tag → `mdy-slash`; any other region → `dmy-slash`.

- [ ] **Step 2: Run** `cd frontend && npx vitest run tests/format.test.ts` → FAIL (functions missing / signature).

- [ ] **Step 3: Implement in `format.ts`:**

```ts
export type DateFormat = 'dmy-dot' | 'dmy-slash' | 'mdy-slash' | 'iso';
export type DateFormatPref = 'auto' | DateFormat;
export const DATE_FORMATS: DateFormatPref[] = ['auto', 'dmy-dot', 'dmy-slash', 'mdy-slash', 'iso'];

const pad = (n: number) => String(n).padStart(2, '0');

/** Pure string work on the calendar date: no Intl and no timezone, so a stored day can never
 *  shift by one and every device shows the same digits for the same choice. */
export function fmtDate(iso: string | null | undefined, format: DateFormat): string {
  if (!iso) return '';
  const [y, m, d] = iso.slice(0, 10).split('-');
  switch (format) {
    case 'dmy-dot': return `${d}.${m}.${y}`;
    case 'dmy-slash': return `${d}/${m}/${y}`;
    case 'mdy-slash': return `${m}/${d}/${y}`;
    case 'iso': return `${y}-${m}-${d}`;
  }
}

export function parseDate(text: string, format: DateFormat): string | null {
  const parts = text.trim().split(/[./-]/);
  if (parts.length !== 3 || parts.some((p) => !/^\d+$/.test(p))) return null;
  let [a, b, c] = parts.map(Number);
  let y: number, m: number, d: number;
  if (format === 'iso') [y, m, d] = [a, b, c];
  else if (format === 'mdy-slash') [m, d, y] = [a, b, c];
  else [d, m, y] = [a, b, c];
  const yearDigits = (format === 'iso' ? parts[0] : parts[2]).length;
  if (yearDigits === 2) y += 2000;
  else if (yearDigits !== 4) return null;
  const date = new Date(Date.UTC(y, m - 1, d));
  if (date.getUTCFullYear() !== y || date.getUTCMonth() !== m - 1 || date.getUTCDate() !== d) return null;
  return `${y}-${pad(m)}-${pad(d)}`;
}

export function resolveDateFormat(pref: DateFormatPref, locale: 'en' | 'de', languages: readonly string[]): DateFormat {
  if (pref !== 'auto') return pref;
  if (locale === 'de') return 'dmy-dot';
  const tag = languages.map((l) => l.split('-')).find(([lang, region]) => lang.toLowerCase() === 'en' && region);
  return !tag || tag[1].toUpperCase() === 'US' ? 'mdy-slash' : 'dmy-slash';
}

export function datePlaceholder(format: DateFormat, locale: 'en' | 'de'): string {
  const [D, M, Y] = locale === 'de' ? ['TT', 'MM', 'JJJJ'] : ['DD', 'MM', 'YYYY'];
  switch (format) {
    case 'dmy-dot': return `${D}.${M}.${Y}`;
    case 'dmy-slash': return `${D}/${M}/${Y}`;
    case 'mdy-slash': return `${M}/${D}/${Y}`;
    case 'iso': return `${Y}-${M}-${D}`;
  }
}
```

(`let [a, b, c]` may need `const` to satisfy lint — keep behaviour.)

- [ ] **Step 4: Settings and store.** `settings.ts`: `LocalSettings` gains `dateFormat: DateFormatPref`, default `'auto'`. Check `persisted()` merges defaults into an older stored object (read `frontend/src/stores/persisted.ts`); if it does not, make the read `{ ...defaults, ...stored }` and add a Vitest case that a stored `{ locale: 'de', theme: 'dark' }` yields `dateFormat: 'auto'`. Create `frontend/src/stores/date-format.ts`:

```ts
import { derived, type Readable } from 'svelte/store';
import { settings } from './settings';
import { browserLang, locale } from '../i18n';
import { navigatorTags } from '../i18n/detect';
import { resolveDateFormat, type DateFormat } from '../lib/format';

/** `browserLang` is only a dependency so a `languagechange` re-resolves the English region. */
export const dateFormat: Readable<DateFormat> = derived([settings, locale, browserLang], ([$s, $l]) =>
  resolveDateFormat($s.dateFormat ?? 'auto', $l, navigatorTags(globalThis.navigator)));
```

- [ ] **Step 5: Replace every `fmtDate(x, $locale)` with `fmtDate(x, $dateFormat)`** in the files listed above (import `dateFormat` from the store). Leave `lastActivityLabel`, `insights.ts`, `stats.ts`, and `Settings.svelte`'s build date (date and time, `Intl`) unchanged. `npm run check` must pass with no remaining `fmtDate(..., $locale)`: `grep -rn "fmtDate(.*\$locale" frontend/src` prints nothing.

- [ ] **Step 6: Appearance setting.** Under the language select in `Appearance.svelte` add a select bound to `$settings.dateFormat`, `aria-label={$t('settings.date-format')}`, options for each id in `DATE_FORMATS`. Labels: `auto` → `$t('settings.date-format-auto', { example: fmtDate('2026-09-15', resolvedAuto) })`; others show `fmtDate('2026-09-15', id)`. Strings: en `'settings.date-format': 'Date format'`, `'settings.date-format-auto': 'Automatic ({example})'`; de `'settings.date-format': 'Datumsformat'`, `'settings.date-format-auto': 'Automatisch ({example})'`.

- [ ] **Step 7: `DateInput.svelte`.**

```svelte
<script lang="ts">
  import { t, locale } from '../i18n';
  import { dateFormat } from '../stores/date-format';
  import { datePlaceholder, fmtDate, parseDate } from './format';
  import Icon from './Icon.svelte';

  let { id, value = $bindable(''), min, max, required = false }:
    { id: string; value?: string; min?: string; max?: string; required?: boolean } = $props();

  let text = $state(fmtDate(value, $dateFormat));
  let invalid = $state(false);
  let picker: HTMLInputElement;

  // An outside change (a picked photo date, a reset form) must show in the field; a change this
  // component made itself already matches, so typing is never overwritten mid-edit.
  $effect(() => {
    const shown = parseDate(text, $dateFormat);
    if ((value || null) !== shown) { text = fmtDate(value, $dateFormat); invalid = false; }
  });

  function commit() {
    if (text.trim() === '') { value = ''; invalid = false; return; }
    const parsed = parseDate(text, $dateFormat);
    const outOfRange = parsed !== null && ((min && parsed < min) || (max && parsed > max));
    invalid = parsed === null || !!outOfRange;
    if (!invalid && parsed) { value = parsed; text = fmtDate(parsed, $dateFormat); }
  }

  function openPicker() {
    picker.value = value;
    try { picker.showPicker(); } catch { picker.focus(); }
  }
</script>

<span class="date-input">
  <input {id} type="text" inputmode="numeric" autocomplete="off" {required}
         placeholder={datePlaceholder($dateFormat, $locale)} bind:value={text}
         aria-invalid={invalid} aria-describedby={invalid ? `${id}-error` : undefined}
         onblur={commit} onchange={commit} />
  <button type="button" class="ghost" aria-label={$t('date.pick')} onclick={openPicker}><Icon name="calendar" size={18} /></button>
  <input bind:this={picker} class="picker" type="date" tabindex="-1" aria-hidden="true" {min} {max}
         onchange={() => { value = picker.value; text = fmtDate(picker.value, $dateFormat); invalid = false; }} />
</span>
{#if invalid}
  <span id={`${id}-error`} class="field-error" aria-live="polite">{$t('date.invalid', { example: fmtDate('2026-09-15', $dateFormat) })}</span>
{/if}

<style>
  .date-input { display: flex; gap: var(--space-1); align-items: center; position: relative; }
  .date-input input[type='text'] { flex: 1; min-width: 0; }
  /* Kept in the layout (not display:none) so showPicker() has an anchor to open from. */
  .picker { position: absolute; right: 0; bottom: 0; width: 1px; height: 1px; opacity: 0; pointer-events: none; }
</style>
```

If `Icon` has no `calendar` name, add one to `Icon.svelte`/`icon-names.ts` following the existing icon pattern (check `tests/icons.test.ts` expectations). If `.field-error` does not exist, use the class the forms already use for field errors (grep `field-error\|form-error` in `frontend/src`). Strings: en `'date.pick': 'Choose date'`, `'date.invalid': 'Enter a date like {example}'`; de `'date.pick': 'Datum wählen'`, `'date.invalid': 'Datum wie {example} eingeben'`.

Submitting a form must commit a typed but not yet blurred value: in each form's submit handler the browser blurs the field before the click, so `onblur` covers it; the e2e below proves it.

- [ ] **Step 8: Replace the five native date inputs** with `<DateInput id="…" bind:value={…} min={…} max={…} required={…} />`, keeping each input's current id, bound value, `min`/`max` and `required`. Labels keep `for=` the same id.

- [ ] **Step 9: E2E** `frontend/tests-e2e/26-date-format.spec.ts`:
  1. Sign in fresh; in Appearance choose `15.09.2026`; create an object and an entry through the form typing the date `3.4.2026` in the entry date field; save; the timeline shows `03.04.2026`.
  2. Switch the format to `2026-09-15` (ISO); the same entry shows `2026-04-03` without reload.
  3. Open the entry form, type `31.02.2026` with the `15.09.2026` format active, blur → the error text "Enter a date like 15.09.2026" is visible and linked via `aria-describedby`; saving keeps the user on the form.
  4. Update existing e2e specs that filled native date inputs with `YYYY-MM-DD` so they type in the active format (default English Chromium in Playwright resolves `auto` to `mdy-slash`; check the project's `locale` in `playwright.config.ts` and assert against the actual resolved format). Never loosen what they assert.

- [ ] **Step 10: Verify.** `cd frontend && npm run check && npx vitest run`; `npm run e2e -- 26-date-format 02-lifecycle 15-reading-reminders 18-templates 19-offline-edit 14-settings 03-search`; then full `npm run e2e` (if OOM-killed, run `--project=mobile` and `--project=desktop` separately and say so).

- [ ] **Step 11: Commit** — `feat: a date format setting, used for every date and typed into an own date field`.

---

### Task 2: Tags on entries everywhere

**Files:**
- Investigate and fix: `frontend/src/routes/ActivityForm.svelte`, `frontend/src/lib/api.ts` (`createQueued`, `updateQueued`, `updateQueuedActivity`), `frontend/src/lib/outbox.ts` (replay of `activity.create`/`activity.update`), `src/api/activities.rs` (create/update), `src/sync/apply.rs`, `frontend/src/lib/Timeline.svelte`, `frontend/src/routes/ObjectDetail.svelte`
- Modify: `frontend/src/routes/Search.svelte`, `frontend/src/routes/ObjectDetail.svelte` (initial `?tag=`), `frontend/src/routes/Dashboard.svelte`, `frontend/src/lib/Reminders.svelte`, `src/api/reminders.rs` (`object_tags`), `frontend/src/lib/types.ts`, `docs/openapi.json`
- Test: extend `frontend/tests-e2e/23-tags.spec.ts`; `tests/reminders.rs`

**Interfaces:**
- Consumes: `fmtDate(iso, $dateFormat)` from Task 1 where dates are touched.
- Produces: reminder JSON field `object_tags: string[]` on `GET /reminders/due` rows and `GET /objects/{id}/reminders` rows (everywhere `object_name` is returned); `Reminder.object_tags?: string[]` in `types.ts`; object page reads `?tag=` as the initial timeline tag filter.

- [ ] **Step 1: Reproduce the timeline bug through the UI.** Write Playwright tests in `23-tags.spec.ts` that use only the entry form (no `page.request.post` for the entry):
  - online create: new object → "+ Log" → title "Bremsbeläge vorne", tag `BBV` (type, Enter) → Save → timeline shows chip `BBV` under that entry;
  - online edit: open the entry, add tag `Bremse` → Save → both chips show;
  - offline create: `context.setOffline(true)`, create entry with tag `BBH`, the pending entry shows the chip; `setOffline(false)`, wait for the outbox to flush (existing helpers in `04-offline.spec.ts`/`19-offline-edit.spec.ts` show how), reload → chip still shows;
  - draft via "+ Add files": attach a small file first (see existing attachment e2e helpers), then add a tag and save → chip shows;
  - typing a tag and clicking Save without pressing Enter → chip shows (the tag field commits on blur).
  Run them; record which fail in the report. Then find the cause (read the code paths above; check the stored row via `GET /api/activities/{id}` in the test) and fix it at the source. If all pass, also try: tags on an entry created from a reminder's "done" flow and from "repeat" suggestions, and on an object copy (`tests/copy.rs` covers object copy — check whether copied activities keep `tags`; if not, fix in the copy code and add a Rust test). Report exactly what was broken; do not leave a passing reproduction as "fixed" without naming a cause.

- [ ] **Step 2: Search chips.** In `Search.svelte`, activity hits and object hits render `<TagChips tags={a.tags} onselect={(tag) => go(`/objects/${a.object_id}?tag=${encodeURIComponent(tag)}`)} />` below the hit button (not inside it — a button cannot contain buttons; follow the `.entry-row` pattern from `Timeline.svelte`). Object hits link to `/objects/${o.id}?tag=…`. Confirm the search response types in `types.ts` include `tags` for both.

- [ ] **Step 3: `?tag=` on the object page.** `ObjectDetail.svelte` reads `tag` from the URL query once on load (see how `tab` is read from `?tab=`) and sets it as the timeline's `tagFilter`, with the timeline tab active.

- [ ] **Step 4: Failing Rust test** in `tests/reminders.rs`: an object with tags `["Bremse", "E-Bike"]` and a due reminder → `GET /reminders/due` row has `object_tags == ["Bremse","E-Bike"]`; `GET /objects/{id}/reminders` rows too; an object without tags gives `[]`. Run `cargo test --test reminders` → FAIL.

- [ ] **Step 5: Implement** `object_tags` in `src/api/reminders.rs`: select `o.tags AS object_tags` wherever `o.name AS object_name` is selected, field `#[serde(serialize_with = "crate::domain::tags::serialize_json_text")] pub object_tags: String` (same as `ObjectRow::tags`); update the unit-test struct literal near line 628. Document in `docs/openapi.json`.

- [ ] **Step 6: UI.** `Dashboard.svelte` reminder rows and `Reminders.svelte` rows render `<TagChips tags={r.object_tags ?? []} />` (no `onselect`: plain labels).

- [ ] **Step 7: Verify.** `cargo test --test reminders --test search --test activities`; PostgreSQL same three; `cargo clippy --all-targets -- -D warnings`; `cd frontend && npm run check && npx vitest run && npm run e2e -- 23-tags 03-search 15-reading-reminders 13-shell`.

- [ ] **Step 8: Commit** — `fix: entry tags survive every way of saving, and show in search and beside reminders` (adjust the first clause to the actual cause found).

---

### Task 3: Last done and the title filter

**Files:**
- Create: `src/api/last_done.rs` (or add to `src/api/activities.rs` if routes there are grouped per resource — follow the existing pattern) and register the route
- Modify: `src/api/activities.rs` (`ListQuery.title`, list and count queries)
- Modify: `docs/openapi.json`
- Create: `frontend/src/lib/LastDone.svelte`; modify `frontend/src/routes/ObjectDetail.svelte`, `frontend/src/lib/Timeline.svelte`, `frontend/src/lib/types.ts`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Test: create `tests/last_done.rs`; extend `tests/activities.rs`; create `frontend/tests-e2e/27-last-done.spec.ts`; Vitest for the "since" helper in `frontend/tests/format.test.ts`

**Interfaces:**
- Consumes: `fmtDate(iso, format)`, `dateFormat` store (Task 1); `counter(value, unit, locale)` from `format.ts`.
- Produces: `GET /objects/{id}/last-done` → `LastDone[]` with `{ title: string, occurrences: number, last_date: string, last_counter: number | null, last_activity_id: number }`; `GET /objects/{id}/activities?title=…`; TS `interface LastDone` in `types.ts`; `sinceCounter(current: number | null | undefined, last: number | null): number | null` in `format.ts`.

- [ ] **Step 1: Failing Rust tests** `tests/last_done.rs` (use `tests/common` helpers like `tests/activities.rs` does):
  - entries on one object: "Bremsbeläge vorne" 2026-01-10 counter 1000, "bremsbeläge vorne " 2026-05-12 counter 3420, "Kette" once, "Wash" once, a `reading` category entry "Stand" twice, a deleted "Bremsbeläge hinten" twice (both deleted) → response is exactly one row with `title` = the newest occurrence's title trimmed (`"bremsbeläge vorne"`), `occurrences: 2`, `last_date: "2026-05-12"`, `last_counter: 3420`, `last_activity_id` = id of the May entry.
  - an open reminder titled "KETTE" on the object → "Kette" (occurrences 1) is included; a done reminder does not include a title.
  - order: newest `last_date` first; tie → higher `last_activity_id` first.
  - another user's object → 404; no entries → `[]`.
  - more than 50 qualifying titles → 50 rows.
  Run `cargo test --test last_done` → FAIL.

- [ ] **Step 2: Implement the endpoint.** Owner check with `load_owned_object`. Portable SQL, folding with `LOWER(TRIM(title))` (note: SQLite `LOWER` folds ASCII only; add a test with "ÄRGER" vs "ärger" and, if SQLite does not fold it, fold in Rust instead: fetch `id, title, date, counter_value` of non-deleted non-reading entries ordered by `date DESC, id DESC`, group in Rust by `title.trim().to_lowercase()`, and fetch open reminder titles folded the same way — choose whichever gives identical results on both backends and document why in a comment). Response struct derives `Serialize`; counts are `i64`.

- [ ] **Step 3: Title filter.** `ListQuery` gains `#[serde(default)] pub title: Option<String>`; list and total count apply "trimmed, case-insensitive exact match" consistently with the fold chosen in Step 2 (same helper). Test in `tests/activities.rs`: `?title=BREMSBELÄGE%20VORNE` returns both "Bremsbeläge vorne" entries and nothing else, `X-Total-Count` is 2; combined with `category` narrows further.

- [ ] **Step 4: OpenAPI** documents the endpoint, the `LastDone` schema and the `title` query parameter; `tests/openapi.rs` still passes.

- [ ] **Step 5: Frontend helper** in `format.ts` with Vitest:

```ts
/** Counter distance since the last time; null when either side is unknown or the counter went
 *  backwards (a replaced odometer, a typo) -- a negative "since" would only mislead. */
export function sinceCounter(current: number | null | undefined, last: number | null): number | null {
  if (current === null || current === undefined || last === null) return null;
  const d = current - last;
  return d >= 0 ? d : null;
}
```
Tests: `(4650, 3420) → 1230`, `(null, 3420) → null`, `(4650, null) → null`, `(100, 3420) → null`, `(3420, 3420) → 0`.

- [ ] **Step 6: `LastDone.svelte`** on the Info tab of `ObjectDetail.svelte` (load with `api<LastDone[]>('GET', `/objects/${oid}/last-done`)` when the Info tab opens, like `loadChildren`). Hidden when the list is empty or the request fails. Heading `$t('lastdone.title')`. Each row is a button: `<b>{title}</b>` and a muted line `fmtDate(last_date, $dateFormat)` · `counter(last_counter, object.counter_unit, $locale)` (when not null) · `$t('lastdone.since', { amount: counter(since, object.counter_unit, $locale) })` (when `sinceCounter(object.stats.current_counter, last_counter)` is not null). Tapping calls `onselect(title)`, which in `ObjectDetail` sets the timeline title filter and switches to the timeline tab. Strings: en `'lastdone.title': 'Last done'`, `'lastdone.since': '{amount} ago'`; de `'lastdone.title': 'Zuletzt erledigt'`, `'lastdone.since': 'vor {amount}'`.

- [ ] **Step 7: Timeline title filter.** `ObjectDetail.svelte` gets `titleFilter: string | null` passed to `Timeline.svelte` like `tagFilter` (bindable), shown as a chip `$t('lastdone.filter', { title })` with the existing clear button pattern; `loadActivities` sets `params.set('title', titleFilter)` when present and treats it as `filtered` (no offline cache read or write). Strings: en `'lastdone.filter': 'Title: {title}'`, de `'lastdone.filter': 'Titel: {title}'`. Changing the title filter triggers a reset load (same effect as category/tag).

- [ ] **Step 8: E2E** `27-last-done.spec.ts`: e-bike with counter unit km; create via API two "Bremsbeläge vorne" entries (1000 km, 3420 km) and one "Kette", and a reading at 4650 km; Info tab shows "Last done" with one row containing "Bremsbeläge vorne", the date in the active format, "3,420 km" and "1,230 km ago" (en; assert German by pattern if the project runs de); tapping it shows the timeline with only the two brake pad entries and the title chip; clearing the chip shows all entries again.

- [ ] **Step 9: Verify.** `cargo test --test last_done --test activities --test openapi` on SQLite and PostgreSQL (port 55446); full `cargo test`; clippy; `cd frontend && npm run check && npx vitest run && npm run e2e -- 27-last-done 23-tags 25-offline-cache 11-controls`.

- [ ] **Step 10: Commit** — `feat: a last done overview per repeated title, and a timeline title filter`.

---

### Task 4: New type from the object form

**Files:**
- Modify: `frontend/src/routes/ObjectForm.svelte`, `frontend/src/routes/settings/Types.svelte`, `frontend/src/i18n/en.ts`, `frontend/src/i18n/de.ts`
- Create: `frontend/src/lib/object-draft.ts`
- Test: `frontend/tests/object-draft.test.ts`, extend `frontend/tests-e2e/24-own-types.spec.ts`

**Interfaces:**
- Produces: `saveObjectDraft(returnPath: string, input: ObjectInput): void`, `takeObjectDraft(returnPath: string): ObjectInput | null` (reads and removes), `safeReturnPath(raw: string | null): string | null` in `object-draft.ts`.

- [ ] **Step 1: Failing Vitest** `tests/object-draft.test.ts` (stub `sessionStorage` with a Map-backed object):
  - `safeReturnPath('/objects/new')` → `'/objects/new'`; `'/objects/12/edit'` → same; `'https://evil.example/objects/new'`, `'//evil/objects'`, `'/settings'`, `null`, `'/objects/../settings'` → `null`.
  - save then take returns the same input; a second take returns `null`; take for a different path returns `null`; unreadable JSON returns `null`.
  Run → FAIL.

- [ ] **Step 2: Implement** `object-draft.ts`: key `logb.object-draft`, stores `{ path, input }`; `safeReturnPath` accepts only `^/objects/(new|\d+/edit)$`. All storage access in try/catch.

- [ ] **Step 3: ObjectForm.** The type `<select>` gets a last option `value="__new_type"` labelled `$t('types.new-from-form')`. Selecting it (in `setType`) does not change `input.type`; it calls `saveObjectDraft(currentPath, $state.snapshot(input))` and `go(`/settings/types?new=1&return=${encodeURIComponent(currentPath)}`)`. On mount, `takeObjectDraft(currentPath)` restores the input when present, and a `type` query parameter (a `custom:` key present in `$customTypes` after `loadCustomTypes`) selects that type through `setType` (so the counter unit default applies). Strings: en `'types.new-from-form': '+ New type…'`, de `'types.new-from-form': '+ Neuer Typ…'`.

- [ ] **Step 4: Types page.** Read `new` and `return` from the query on mount: `new=1` opens the add form (`open('new')`). With a valid `safeReturnPath(return)`, a successful create navigates to `${return}?type=${encodeURIComponent(created.key)}` (use the created type's key as returned by the create call / registry), and the form's Cancel navigates to `return` without `type`. Without `return` behaviour is unchanged.

- [ ] **Step 5: E2E** in `24-own-types.spec.ts`: on `/objects/new` type a name "Mein Pedelec", pick "+ New type…", land on Types with the add form open, create "Pedelec" with unit km, return to the object form with name "Mein Pedelec" kept and type "Pedelec" selected and counter unit km; save → object page shows the type. Second case: Cancel on Types returns to the form with the name kept and the previous type.

- [ ] **Step 6: Verify.** `cd frontend && npm run check && npx vitest run && npm run e2e -- 24-own-types 02-lifecycle 12-object-hierarchy`.

- [ ] **Step 7: Commit** — `feat: create an own type straight from the object form`.
