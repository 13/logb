# Reading reminders

Status: implemented.

## Problem

"Log the car's km every month" could be built from a date reminder that repeats monthly, and it
was a poor fit:

- Logging the km at a fuel stop did not clear it. The reading and the "mark done" were two jobs.
- Each completion inserted a new reminder row: twelve closed "Log km" reminders a year.
- Completing late moved the whole schedule to the day it was finally done.
- There was no quick way to record a reading. The full activity form asked for a category and
  title, and no category described "just the counter".

## The model

A reminder has a `kind`: `service` (everything that existed) or `reading`.

A reading reminder stores `every_n` + `every_unit` (`week` | `month`, 1..60) and keeps its start
in `due_date`, so the existing "due_date or due_counter" CHECK holds. It stores **no completion
state**. Its due date is derived on every read:

```
last     = newest activity with a counter_value, dated on or before today, not deleted
next_due = max(start, last + interval)       -- months clamp: 31 Jan + 1 month = 28 Feb
due      = today >= next_due, unless a snooze is live
```

Consequences, each tested in `tests/reading_reminders.rs`:

- Any entry with a counter clears it: the quick reading form, a fuel stop, a service, a device
  syncing an entry made offline.
- Deleting that entry makes it due again. Nothing to keep in step, nothing to sync.
- A backdated reading counts by its own date; a future-dated one (a typo'd year) is ignored.
- `POST /reminders/{id}/done` answers 409 for a reading reminder: `done_at` would stop it for
  good, when what the user means is "I logged it".
- Snooze works unchanged, measured from the derived date. "Skip this one" in the UI is a snooze
  one interval long.
- `kind` never changes after create (PATCH refuses; not in the sync whitelist).

The rule lives once, in `domain::reminder::reading_status`. `ReminderOut` uses it for `due`, and
`objects::due_readings` uses it for `stats.due_reminder_count` — the SQL count covers service
reminders only, because a calendar-month addition is not portable SQL across SQLite and
PostgreSQL.

`activities.category` gains `reading`: an entry that is only a counter value, requires
`counter_value`, and is excluded from the cost-per-category breakdown. The timeline folds readings
into one dashed line each.

## Schema

SQLite `0011_reading_reminders.sql` (activities rebuild for the CHECK, same pattern as 0009; three
`ALTER TABLE reminders ADD COLUMN`). PostgreSQL `0002_reading_reminders.sql` — a new step, not an
edit to `0001`, because released PostgreSQL databases have already applied `0001` and sqlx refuses
a changed checksum. Export/import carry `kind`, `every_n`, `every_unit` with serde defaults, so
older archives import as service reminders.

## Estimated dates

Regular readings make usage measurable. `domain::insights::daily_rate_milli` takes the earliest
reading in the last 180 days to the newest (falling back to the earliest of all when that span is
under 14 days; no rate for a shorter span or a counter that went down). From it:

- `estimated_due_date` on a service reminder with an unreached `due_counter`. It feeds the
  `within_days` lookahead — a mileage-only service now appears under "Coming up" — and never
  makes anything due.
- `counter_per_day_milli` on insights, shown as "≈ N km a month".

## Notifications

The digest lists services as before, then `Readings to log:` with each reading's line. With
`LOGB_PUBLIC_URL` set, every item carries a `link` (JSON) and reading lines end in their form's URL
(text); a text digest about exactly one reminder also sends ntfy's `Click` header.

## UI

- Reminder form: "What to remind you of" — a service/date, or logging the counter reading
  (objects with a counter only; fixed once saved).
- Reminders tab: a reading card shows the interval, last reading and next date, with "Record
  reading" and, when due, "Skip this one". No "Mark done". The empty state offers "Remind me to
  log the reading".
- `/objects/:id/reading`: one number (prefilled, focused) and a date. Saved through the offline
  outbox. A value below the last reading, or more than 5× the recent rate implies (and over
  300 units), asks for a second save instead of refusing.
- New object form: opt-in "Remind me to log the reading every month", starting a month out.
- Dashboard: "Record reading" beside due readings; "≈ date" beside estimated services.
