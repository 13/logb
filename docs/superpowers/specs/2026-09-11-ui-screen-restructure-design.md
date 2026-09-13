# Interface: screens that earn their space

Status: SUPERSEDED by `2026-09-13-consistent-controls-design.md`, which merges both slices and
names the controls neither of them mentioned. Kept as the record of how the work was first
scoped. Second of two slices; depends on the system and
states slice, and reads better after object types land, since the dashboard shows a type icon.

## Problem

The screens are laid out, not designed. The dashboard is the clearest case: an object row
carries a name and a single word, a 44px `+` sits beside it with no label and reads as a second
card, `Show archived` is a bare checkbox loose in the content flow, and below two objects the
remaining 1,400 pixels are empty. A logbook's front page shows a name and the word "car" while
the mileage, the money spent and the overdue service it already knows about stay one tap away.

Object detail has the opposite problem: four stats, four tabs, filter chips and a timeline
arrive at once with nothing establishing which matters. The activity form runs past a phone
screen, so Save is reached by scrolling past everything.

## Scope

Restructuring the dashboard, object detail and the activity form. Same visual identity, same
palette, same components — this slice decides what goes where and what a screen says when you
arrive at it.

Out of scope: new capabilities. Nothing here adds a feature, changes what is stored, or alters
what the app can do. Settings and search keep their structure.

## The dashboard

An object row shows what the app already knows and currently hides: its type icon, its name,
its counter reading, its total cost, when it was last touched, and whether a reminder is due.
Due is the one thing that should find the user rather than wait to be found, so it reads as a
status on the row rather than a number to decode.

The unlabelled `+` becomes an explicit quick-log affordance. It is the app's most-used action —
log something against this object — and it should say so.

`Show archived` becomes a filter consistent with the chips the object detail already uses,
rather than a checkbox in the middle of the content.

The type icon does real work here: a list of six objects is scanned by shape before it is read.

## Object detail

The screen answers three questions in order: what is this, what has it cost and when was it
last serviced, and what has happened to it. The stats row leads, the tabs follow, and the
timeline is the body.

Timeline entries are already grouped by year. Within a group, an entry's date, category and
figures line up in a column — the app sets figures in tabular numerals for exactly this, and
the current layout does not take advantage of it.

## The activity form

The form is long because every field is always present. The fields that are always filled —
date, category, title — lead; cost, counter, quantity and notes follow; photos and documents
close. Save is reachable without scrolling past the whole form on a phone.

The quantity field already appears only for fuel entries. That pattern — a field that appears
when it applies — is the one to follow rather than an accordion, which hides fields behind a
tap and a guess.

## What this deliberately does not do

No new screens, no navigation change, no feature. A reviewer comparing before and after should
see the same application, with each screen saying more about what it is for.

## Testing

- The dashboard row shows a due reminder without opening the object: seeded, asserted
  end-to-end.
- Quick-log from the dashboard reaches the activity form for the right object, and saves.
- The archived filter shows and hides archived objects, and its state is not lost on
  navigation.
- The activity form's Save is reachable on a Pixel 7 viewport without scrolling past the
  photos section — a measurement, since that is the complaint.
- Screens are verified by eye against before-and-after screenshots in both themes.
