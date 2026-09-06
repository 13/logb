# memto: daily-use improvements

Date: 2026-09-06
Branch base: `improvements` (feeb1d3)

## Goal

memto works. What it does not yet do is get out of the way. This design targets the
friction of using it every day as its own owner — fewer taps and less typing to log
something, answers to "what has this cost me", and a log that survives a garage with
no signal.

Explicitly out of scope: multi-user features, public-release polish, multi-arch images.
Those are worth doing; they are not what this batch is for.

## Friction inventory

Found by reading the current flows:

1. Logging an activity takes five taps minimum — dashboard, object, tab, FAB, form.
2. The title is free text, retyped every time. "Fuel", "Oil change" have no memory.
3. Photos need a saved parent, so `ensureSaved()` creates a real activity mid-form.
   Cancelling afterwards leaves that activity orphaned in the timeline. This is a bug.
4. `FilePicker` has no `capture` attribute, so a phone opens the file browser rather
   than the camera.
5. The dashboard banner shows only what is already overdue. Nothing warns beforehand.
6. Cost is a single number per object. No breakdown by year or category, no cost per
   km, though the counter data to compute it is already stored.
7. The `fuel` category records no quantity, so consumption cannot be derived.
8. Every write needs the network. The service worker caches the shell; a log attempt
   with no signal fails outright.

## Phases

Three phases, each independently shippable, in this order.

| Phase | Theme | Chunks | Surface |
|-------|-------|--------|---------|
| A | Fast capture | 1 | frontend, one read-only endpoint |
| C | Cost insight | 2 | migration, insights API, dashboard and object UI |
| B | Offline | 2 | idempotency columns, outbox module, e2e |

Order rationale: A is small and pays off on every single log. C lands before B because
its schema change (fuel quantity) alters the activity payload — queueing that payload
offline before the shape is final would leave stored ops that no longer match the API.
B is last, largest, and depends on the idempotency key, which only makes sense once the
payload is settled.

## Phase A — fast capture

### A1. Quick-log from the object card

`ObjectCard` gains a `+ log` button navigating straight to
`/objects/:id/activities/new`. Cuts dashboard → detail → FAB down to one tap.

### A2. Recent-title suggestions

New endpoint:

```
GET /objects/{id}/recent-titles
→ [{ "title": "Fuel", "category": "fuel", "last_date": "2026-08-30",
     "last_cost_cents": 6210, "last_counter": 41230 }]
```

Up to 20 rows, distinct on `(title, category)`, most recent first. A dedicated endpoint
rather than reusing `GET /objects/{id}/activities`: that one joins every attachment for
the object, which is payload a phone does not need in order to fill a datalist.

`ActivityForm` renders these as a `<datalist>` on the title input, narrowed to the
selected category when one is chosen.

### A3. Repeat-last chips

The top three suggestions render as buttons above the form. Tapping one prefills title,
category and cost. Prefill only — the user still reviews and saves. The date and counter
are already prefilled today and stay as they are.

### A4. Camera capture

`FilePicker` gains a second hidden input with `capture="environment"` and its own
button, next to the existing file button. `capture` cannot go on the shared input: on
mobile it suppresses picking an existing file.

### A5. Fix the abandoned draft

`ensureSaved()` inserts a real activity so uploaded files have a parent. When the user
then cancels, that activity stays. Fix: track whether the row was auto-created for
uploads and never explicitly saved; on cancel, delete it (and with it, through the
existing cascade, its attachments). Confirm first if it already has uploads attached.

## Phase C — cost insight

### C1. Schema

Migration `0003`:

```sql
ALTER TABLE activities ADD COLUMN quantity_milli INTEGER;
ALTER TABLE objects ADD COLUMN fuel_unit TEXT CHECK (fuel_unit IN ('l', 'gal', 'kwh'));
```

`quantity_milli` is the fuel amount scaled by 1000, integer like `cost_cents` — the
database stores no floats. `fuel_unit` null means derive from `counter_unit`
(`km` → `l`, `mi` → `gal`, `h` → `l`); it is explicit so an e-bike can record `kwh`.

`ActivityInput` accepts `quantity_milli` and validates it: non-negative, and only on
objects that have a counter unit. It is shown in the form only for category `fuel`.

### C2. Insights endpoint

```
GET /objects/{id}/insights
→ {
    "by_year":     [{ "bucket": "2026", "cost_cents": 48000, "count": 7 }],
    "by_category": [{ "bucket": "fuel", "cost_cents": 31000, "count": 4 }],
    "counter_span": { "from": 12000, "to": 31230 },
    "cost_per_counter_milli": 2496,
    "fuel": { "unit": "l", "quantity_milli": 184300,
              "per_100_milli": 7300, "cost_per_counter_milli": 980 }
  }
```

`cost_per_counter_milli` is cents per counter unit scaled by 1000; null when fewer than
two counter readings exist, since the span would be zero.

`fuel` is null unless at least two fuel activities carry both a counter value and a
quantity. Consumption uses the standard tank method: sum the quantities of every fill
*except the first*, divide by the counter span from the first fill to the last. The
first fill establishes the starting point and its fuel was burned before the window.

The whole `fuel` block describes that one window. `fuel.cost_per_counter_milli` is
therefore the cost of the same fills, the earliest one excluded exactly as its quantity
is, divided by the same fuel span — not by the object's overall counter span, and not
over fuel rows that lack a counter or a quantity. A fill with no recorded cost counts as
a fill and contributes nothing to the numerator. When consumption is unmeasurable, the
cost figure is null too: the two live or die together.

This was settled during implementation. Dividing fuel-only cost by the whole-object span
put two different windows in one JSON object — an object whose last activity was a
repair measured its consumption over one distance and its fuel cost over another.

All of this arithmetic lives in `src/domain/insights.rs` as pure functions over plain
inputs, unit-tested there; the handler does SQL and assembly only. This mirrors the
existing split with `src/domain/reminder.rs`.

### C3. Upcoming reminders and snooze

`GET /reminders/due?within_days=N` — `N` defaults to 0, so the notification digest and
the current dashboard call keep their behavior exactly. Each item gains:

- `due: bool` — already computed today
- `days_until: number | null` — negative when overdue
- `counter_until: number | null` — counter distance remaining

`POST /reminders/{id}/snooze { "days": 7 }` sets a new `snoozed_until` column to `days`
after the later of today and the current `due_date` — snoozing means "not now, in a
week", so a reminder three months overdue lands a week out, not three months in the past
plus seven days. `is_due` returns false while `snoozed_until` is strictly after today,
and the snooze lapses on that date. Rejects a reminder that is already done (409) and
`days` outside 1..365 (400).

Snooze *suppresses*; it does not rewrite `due_date`. This was settled during
implementation, replacing an earlier design that pushed `due_date` forward. Rewriting the
date cannot suppress a reminder that is due by counter — `is_due` is `by_date ||
by_counter`, so the row stayed due and the button visibly did nothing, in exactly the
case you would reach for it: the odometer is past the service interval and the garage is
next week. Suppressing also keeps the real due date intact, so `days_until` stays
truthful while the reminder is hidden.

Every count of what is due shares that gate, including the object's `due_reminder_count`,
which drives the red badges on the dashboard card and the reminders tab. A snooze that
worked on one screen and not another would be worse than none.

### C4. UI

- Dashboard banner splits into two groups: overdue in the accent color, upcoming muted
  and phrased as "in 12 days" / "in 400 km". Snooze button per row.
- The object's Info tab renders the rollups: a year list, a category breakdown as CSS
  bars, cost per counter unit, and consumption when `fuel` is present.

No chart library. Bars are divs with a width percentage; the bundle stays small and the
existing `money()` / `counter()` formatters do the labels.

## Phase B — offline

### B1. Idempotency

Migration `0005`:

```sql
ALTER TABLE activities ADD COLUMN client_op_id TEXT;
CREATE UNIQUE INDEX idx_activities_client_op
  ON activities(client_op_id) WHERE client_op_id IS NOT NULL;
ALTER TABLE attachments ADD COLUMN client_op_id TEXT;
CREATE UNIQUE INDEX idx_attachments_client_op
  ON attachments(client_op_id) WHERE client_op_id IS NOT NULL;
```

When a create carries a `client_op_id` that already exists, the handler returns the
existing row with 200 instead of inserting and returning 201. Without this, a response
lost on a flaky connection makes the client replay an op that in fact succeeded, and
the user gets the same fill-up logged twice.

The attachment id travels as a multipart field alongside the file.

### B2. Outbox

`frontend/src/lib/outbox.ts`. Queue logic is pure and sits behind a small storage
adapter interface: vitest drives an in-memory adapter, production uses IndexedDB.

Operations, creates only:

- `activity.create`
- `attachment.upload` (carries the blob)
- `reminder.done`

Edits and deletes require the network. This is a deliberate limit — it removes conflict
resolution from the design entirely. A queued create cannot disagree with the server
about anything; a queued edit can.

Rules:

- Enqueue only on a genuine network failure — `fetch` throws, or `navigator.onLine` is
  false. An HTTP 4xx is a rejection, not an outage: surface it and drop the op.
- New records get a temporary negative id. An `attachment.upload` naming a temp activity
  id has it rewritten once the create returns the real one.
- Replay serially in insertion order, triggered on the `online` event and at startup.
- An op that fails three times moves to a dead list, surfaced in Settings. Nothing
  disappears silently.
- `TopBar` shows a pending count; optimistic rows render dimmed in the timeline.

## Testing

- **Rust unit** — `domain/insights.rs`: consumption with one fill, with a zero span,
  with quantities but no counters, with the first fill excluded correctly. Snooze date
  math across a month boundary.
- **Rust integration** — `recent-titles` ordering and distinctness; `insights` against a
  seeded object; snooze guards; `within_days` filtering; a replayed `client_op_id`
  returning the same id with 200 and leaving the row count unchanged.
- **vitest** — outbox ordering, temp-id resolution, poison path, and the enqueue-only-on-
  network-error rule; title suggestion filtering by category.
- **Playwright** — one offline run: `context.setOffline(true)`, log an activity, go back
  online, assert it appears exactly once.

## Open question

`fuel_unit` is per object. The alternative is instance-wide, like `currency` already is
(see feeb1d3). Per object wins here because one instance plausibly holds both a petrol
car and an e-bike, whereas one household has one currency.
