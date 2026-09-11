# Object types

Status: approved design, not yet implemented.

## Problem

An object's `category` is free text (`objects.category TEXT NOT NULL`), typed by hand with a
datalist suggesting `car`, `e-bike`, `bike`, `home`, `tool`, `motorcycle`. Nothing can rely on
it: `Fahrrad`, `bike` and a typo are three different values. So every object looks alike in a
list, and the activity vocabulary is the same for all of them — a dishwasher is offered
*Fuel / charge*, and there is no sensible way to log that a shoulder hurt on a Tuesday.

The app already tracks anything with a maintenance history. What it cannot do is *know what
kind of thing* it is tracking.

## Scope

A fixed object type replacing the free-text category; an icon per type; and an activity
category vocabulary that adapts to the type, including first-class support for logging a body.

Out of scope: per-type fields (a car's number plate, a medication's dose), reminders keyed to
type, and any change to how counters or costs work.

## Types

Nine, fixed: `car`, `e_bike`, `bike`, `motorcycle`, `home`, `appliance`, `tool`, `body`,
`other`. Stored as a `TEXT` column with a `CHECK`, the way `activities.category` already is —
the constraint is what makes the value trustworthy enough to key behaviour off.

Each type carries an icon and a set of activity categories. Both live in one table in the
frontend, so adding a type is one entry rather than edits scattered across screens.

## Migration

`objects.category` is replaced, not kept alongside — two fields that both describe what a
thing is would drift apart within a week.

Existing values map case-insensitively, trimmed, in both languages the app speaks:

| Type | Matches |
|---|---|
| `car` | car, auto, pkw, wagen |
| `e_bike` | e-bike, ebike, e bike, pedelec |
| `bike` | bike, fahrrad, velo, rad |
| `motorcycle` | motorcycle, motorrad, motorbike |
| `home` | home, haus, wohnung, flat, apartment |
| `appliance` | appliance, gerät, geraet, haushaltsgerät |
| `tool` | tool, werkzeug, maschine |
| `body` | body, körper, koerper, health, gesundheit |

`e_bike` is matched before `bike`, or every e-bike becomes a bike.

Anything unmatched becomes `other` **and its text is appended to the object's description**, on
its own line, so nothing the user typed is destroyed by a migration they did not ask for. An
empty description gets the text alone.

No fuzzy or substring matching. `Gravelbike Custom` becoming a `bike` by accident is a silent
mis-filing with no record that a guess was made; landing on `other` with the words preserved is
visible and correctable in one edit.

## Activity categories

The seven existing values stay and four join them: `symptom`, `treatment`, `appointment`,
`medication`. One global vocabulary — a category means the same thing on every object — so
`by_category` cost grouping and search continue to mean what they meant.

SQLite cannot alter a `CHECK` in place, so both tables are rebuilt the standard way — create
the new table, copy the rows, drop the old, rename — inside the migration's transaction. The
same applies to `objects`, which gains a constrained column and loses an unconstrained one.
Foreign keys pointing at `objects` and `activities` must survive the swap; `activities`,
`attachments` and `reminders` all reference one of them.

Each type offers a subset:

| Type | Offers |
|---|---|
| `car`, `motorcycle` | maintenance, repair, inspection, fuel, modification, purchase, other |
| `e_bike` | maintenance, repair, inspection, fuel, modification, purchase, other |
| `bike` | maintenance, repair, inspection, modification, purchase, other |
| `home`, `appliance`, `tool` | maintenance, repair, inspection, modification, purchase, other |
| `body` | symptom, treatment, appointment, medication, other |
| `other` | all eleven |

**The picker always includes the entry's current category, even when the type does not offer
it.** Change a car to `other` and back, or re-type an object after logging against it, and the
history stays readable. Without this rule, editing an old entry would silently re-file it under
whatever the select happened to land on — the kind of quiet data change that is discovered
months later.

Filtering is presentation only. Nothing hides or rewrites rows whose category is outside the
current type's set; the timeline's filter chips continue to show every category actually
present on the object.

## Icons

Nine variants added to the existing `frontend/src/lib/Icon.svelte`: same inline SVG, 24×24
viewBox, `currentColor`, `stroke-width="1.75"`, no runtime dependency. They appear on dashboard
rows, in the object detail top bar, in search results, and in the type picker itself, so the
choice is visible while it is being made.

## Sync

The new column must be added to the field whitelist as `FieldType::Text`, and the removed one
taken out. A field missing from that whitelist is bound as NULL, reported to the client as
accepted, and advances `field_clock` — the write is lost and cannot be repaired by retrying.
This project has shipped that bug twice; it is the first thing to check in review, not the
last.

A second case belongs to sync rather than the schema: a client that was offline across the
upgrade can arrive with a queued operation naming the removed field. That operation must be
rejected individually and reported as rejected, not fail the batch — a batch that 500s is
retried identically forever, which is how a single bad operation once wedged a client
permanently. The same handling already exists for constraint violations and extends to an
unknown field name.

## Export and import

`data.json` carries object rows, so an archive written before this change contains `category`
and one written after contains `type`. Import applies the same mapping table as the migration,
including the description fallback, so an old backup restores into the new model without
landing every object on `other`.

## Testing

- The mapping table is a pure function and gets unit tests, including the `e_bike`-before-`bike`
  ordering, case and whitespace handling, both languages, and the unmatched case appending to a
  description that is empty and to one that is not.
- A migration test: rows with known, unknown, and empty categories, asserted on the other side
  for type, description and preserved identity.
- An end-to-end test creating a `body` object, logging a `symptom`, and asserting the category
  select offers the health vocabulary and not `fuel`.
- A regression test for the current-category rule: an activity whose category is outside its
  object's type set still appears in the select and survives an edit that does not touch it.
- A sync test asserting a type change round-trips, since the whitelist is the known trap.
