# Objects list: tabs, search, sorting, richer cards

Status: implemented. First of three projects (objects list → tags → own types).

The dashboard's object list is a fixed name-ordered list of top-level objects with a "Show
archived" chip. Finding one object among many means scrolling or leaving for the Search screen,
there is no way to see what was used recently, and a card says a vehicle's kilometres but not how
much it is used or when it was last touched.

## Screen

Below the reminder banners, top to bottom:

1. **Tabs: Active | Archived**, each with its count ("Active 12"). They replace the "Show
   archived" chip.
2. **Search box** ("Search objects"). Filters as you type, case- and accent-insensitive, on name,
   type label (in the reader's language) and description. Session-only: not in the URL, not
   remembered.
3. **Sort** select: Name A–Z (default), Last activity, Recently changed, Highest cost, Highest
   counter.
4. **List.**
   - Active tab, no search: top-level objects -- an object whose parent is not among the active
     objects counts as top-level, so a live child of an archived parent is still reachable.
   - Active tab while searching: matches at any depth; a nested match shows "in House" under its
     name, the idiom the Search screen uses.
   - Archived tab: every archived object at any depth, flat (as today), with "in …" when its
     parent is known.
   - Sorting applies to whatever is shown. An object without the value sorted by (no entries, no
     counter, no cost) goes last; ties and missing values fall back to name.
   - Nothing matches: "No objects match “{query}”."

Tab and sort live in the URL (`?tab=archived&sort=last-activity`, default values left out,
replaced only when they change), so reload and the back button keep them. The sort is also
remembered per device (`logb.objects.sort`); the URL wins over the remembered value.

Both lists load once on open: `GET /objects?all=true&archived=false` and
`GET /objects?all=true&archived=true`. Switching tabs, typing and sorting make no requests. A
household has tens of objects; this is cheaper than any server-side paging would be.

## Card

The existing line gains two items, each only when it applies:

- **Usage per month**: "≈ 516 km a month", from `stats.counter_per_day_milli` × 30.44 ÷ 1000,
  rounded to whole units -- the Info tab's figure and rule.
- **Last activity**: "today", "yesterday", "3 days ago" up to 30 days, then the month and year
  ("Mar 2025"), from `stats.last_activity_date`.

Order: type · counter · usage · cost · last activity.

## Server

`ObjectStats` gains, for every object read and list:

- `last_activity_date`: the newest non-deleted activity dated today or earlier. A future-dated
  entry (a planned expense) is not "recent activity".
- `counter_per_day_milli`: the usage rate from `insights::usage_by_object`, the helper reminders,
  insights and the reading form already use -- one query for all of a user's objects in the list.
  `usage_by_object` takes `Option<i64>` for the user so the single-object paths, which have
  already checked ownership, can call it too.

No schema change, no new endpoint. `docs/openapi.json` describes the two fields.

## Structure

- `frontend/src/lib/object-list.ts` (pure): `SortKey`, `parseSort`, `matchesQuery`,
  `sortObjects`, `visibleRows` (roots vs. search vs. archived, with parent names).
- `frontend/src/lib/format.ts`: `lastActivityLabel(date, today, locale)`.
- `Dashboard.svelte`: tabs, search, sort, list of `visibleRows`.
- `ObjectCard.svelte`: optional `parentName` prop, usage and last-activity items.

## Tests

- Rust (`tests/object_list.rs`, SQLite and PostgreSQL): both stats fields on list and single
  read; a future-dated entry ignored for `last_activity_date`; rate null without history.
- Vitest: `object-list.ts` (each sort with missing values and ties, accent/case-insensitive
  search across name/type/description, roots including children of archived parents, parent
  names), `lastActivityLabel` (today, yesterday, days, month-year; German by pattern).
- Playwright (`22-objects-list.spec.ts`): tabs with counts; search finds a nested object with "in
  …"; last-activity sort order; sort survives reload; card shows usage and last activity.
  Existing specs that click "Show archived" move to the Archived tab.

## Out of scope

Tags (next project), user-defined types (after that), server-side paging.
