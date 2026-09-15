# Date format, tags on entries, "last done", own types shortcut

Status: approved, not implemented.

## A. Date format

**Setting.** Settings > Appearance gains "Date format" with options `auto`, `dmy-dot` (15.09.2026),
`dmy-slash` (15/09/2026), `mdy-slash` (09/15/2026), `iso` (2026-09-15). Stored per device in the
existing `logb.settings` local store next to `locale` and `theme` (`dateFormat`, default `auto`).
Old stored settings without the field read as `auto`.

`auto` resolves as: app language `de` → `dmy-dot`; app language `en` → the browser's first language
tag with a region (`navigator.languages`): `en-US` (or no region) → `mdy-slash`, any other `en-XX`
region → `dmy-slash`.

**Formatting.** A single `fmtDate(iso, format)` in `frontend/src/lib/format.ts` renders a
`YYYY-MM-DD` (or ISO timestamp, first 10 chars) as the chosen pattern with zero-padded day and month.
It is pure string work (no `Intl`, no timezone). A derived store `dateFormat` (resolved, never
`auto`) is what components pass. Every full-date display uses it: timeline, readings fold, reminders
(due, snoozed, done, estimated, last/next reading), search hits, dashboard estimates, reading form
hint, "use photo date" hint, backup "last snapshot", API token "last used". Month-only and relative
labels (`lastActivityLabel`, `insights.ts`, `stats.ts` month names, "today / 3 days ago") keep using
`Intl` with the app language.

**Date field.** A `DateInput.svelte` component replaces every `<input type="date">` (entry form,
reading form, reminder form start/due, object purchase date). It shows a text input with the chosen
pattern as placeholder (`TT.MM.JJJJ` / `DD.MM.YYYY` etc. in the app language), `inputmode="numeric"`,
and a calendar button that opens a hidden native `<input type="date">` via `showPicker()` (fallback:
focus it) and writes the picked value back. It binds a `YYYY-MM-DD` string (or empty) like the native
input did, and honours `min`/`max`/`required`. Parsing is pure (`parseDate(text, format)` in
`format.ts`): accepts the chosen pattern, also `.`/`/`/`-` separators and 1-digit day/month, and a
2-digit year as 20YY; rejects impossible dates (31.02.). An unparsable value shows
`date.invalid` ("Enter a date like {example}" / "Datum wie {example} eingeben") under the field with
`aria-describedby`, and the form's own validation treats the field as empty/invalid as before.

## B. Tags on entries

1. **Object timeline.** Reproduce first through the real entry form (create online, create offline
   then sync, edit online, edit offline then sync, draft created by "+ Add files" then saved) and fix
   the cause found. A Playwright test drives the form (not the API) and asserts the chips on the
   timeline for create and edit.
2. **Search results.** Activity hits render their `tags` with `TagChips`; tapping a chip goes to
   `/objects/{object_id}?tag={tag}`, and the object page applies a `tag` query parameter as the
   timeline's initial tag filter. Object hits show their tags the same way.
3. **Dashboard and reminders.** Reminder rows (dashboard due list and the object's reminders tab)
   show the object's tags as non-interactive chips. The dashboard reminder API responses gain
   `object_tags` (array) where they carry `object_name`.

## C. "Last done"

**Endpoint.** `GET /objects/{id}/last-done` (owner only, 404 otherwise) returns
`[{ title, occurrences, last_date, last_counter, last_activity_id }]`: activities of the object that
are not deleted and not category `reading`, grouped by title folded for case (`LOWER(TRIM(title))`),
kept when `occurrences >= 2` or an open (not done) reminder of the object has the same folded title.
`title` is the spelling of the newest occurrence; `last_*` come from the newest occurrence (by date,
then id). Sorted by `last_date` descending, at most 50 rows. Portable SQL, tested on SQLite and
PostgreSQL. Documented in `docs/openapi.json`.

**UI.** The object Info tab shows a "Last done" / "Zuletzt erledigt" section (hidden when empty):
each row `title · date · counter · since`, where since is `current_counter - last_counter` formatted
with the object's unit ("vor 1.230 km" / "1,230 km ago"), omitted when either counter is missing or
the difference is negative. Tapping a row switches to the timeline with a title filter.

**Title filter.** `GET /objects/{id}/activities` gains `title` (exact match ignoring case and
surrounding spaces). The timeline shows an active title filter like the tag filter (chip with a clear
button), combinable with category and tag. The offline activity cache is not used for a
title-filtered load, same as for category and tag.

## D. Own types shortcut

The type `<select>` in the object form ends with "+ New type…" / "+ Neuer Typ…". Choosing it
navigates to `/settings/types?new=1&return=/objects/new` (or the edit URL), with the form's current
input kept in `sessionStorage`. Types opens the add form directly when `new=1`; after a successful
save it returns to `return` with `type=custom:<uuid>` and the object form restores its kept input and
selects that type. Cancel returns without changing the type. `return` must be a same-app path
starting with `/objects/`.

## Out of scope

Server-side date formatting (webhook/push digests keep their current text), per-user (server-stored)
date format, a title rename tool.
