# Interface: system adoption and states

Status: approved design, not yet implemented. First of two slices; the second restructures
screens on top of this one.

## Problem

The design-foundations slice built a token system and stopped at `app.css`. Component
`<style>` blocks still carry roughly fifteen improvised font sizes — `.6`, `.75`, `.8`, `.85`,
`.9`, `1`, `1.4rem`, the exact list the foundations spec complained about — and around forty
spacing literals, including off-grid `6px`, `10px` and `18px`. Two systems now sit side by
side, which is worse than one improvisation: a reader cannot tell which is authoritative.

Separately, the app has no designed states. Every screen has exactly one appearance: the one
with data in it.

- An account with no objects gets a heading and blank space where an invitation should be.
- A tab with no activities, no documents and no reminders each show nothing at all.
- A search with no results shows nothing, which is indistinguishable from a search that has
  not run.
- Nothing indicates loading. On a slow connection a populated screen and an empty one look
  identical until the data lands.

## Scope

Adopting the existing tokens throughout the components, and giving every screen its empty and
loading appearance. One definition each for the three components that currently have several:
button, chip and card.

Out of scope, and next: the dashboard's structure, object detail density, and the activity
form's length. Structure is the second slice; this one makes the pieces consistent so that the
restructure has a vocabulary to build with.

No palette change. No new dependency.

## Token adoption

Every literal font size in a component `<style>` block resolves to a step of the type scale;
every spacing literal to a step of the space scale. Off-grid values round to the nearest step
rather than gaining new tokens — `6px` becomes `--space-1` or `--space-2` by which side of the
gap it serves, and `18px` becomes `--space-4`.

Where a literal turns out to encode something real that the scale cannot say — an icon's
optical alignment, a hairline — it stays, with a comment saying why. A token scale that cannot
express a thing is not improved by a token that lies about it.

## States

Every empty state is a sentence and an action, in the interface's voice, never an apology and
never a bare "No data".

| Screen | Empty state |
|---|---|
| Dashboard | What the app is for, and the button that starts it |
| Timeline | An invitation to log the first entry for this object |
| Documents | What belongs here — receipts, manuals, photos — and the picker |
| Reminders | What a reminder does, and the button to add one |
| Search | What was searched for, and that nothing matched it |

Loading is a skeleton of the content that is coming, not a spinner: the list's shape, the
card's shape. A spinner says "wait"; a skeleton says "a list is arriving", and it does not move
the page when the data lands. It respects `prefers-reduced-motion`, which the foundations slice
already honours globally.

An error state is not new work here — the app already surfaces failures — but errors must use
the same component as the other states rather than a bare red paragraph.

## Components

Button, chip and card each get one definition and one set of variants. Today a button is styled
in `app.css` and re-styled in several component blocks; a chip means both a filter control and
a status badge. The variants that survive are the ones in use: button `primary`, `ghost`,
`danger`; chip as a filter and as a status; card as a container.

## Testing

Visual consistency is not unit-testable, and asserting computed CSS proves only that the file
says what the file says. What is worth testing is behaviour that is absent today and that a
regression would silently remove:

- Each empty state renders its action, and the action works — an end-to-end test per screen,
  seeded with nothing rather than mocked.
- A search with no matches is distinguishable from a search that has not run.
- A test that fails if a component `<style>` block reintroduces a raw font size, in the same
  spirit as the icon test: a regression guard for a decision that is otherwise invisible.

Everything else is verified by eye against screenshots in both themes.
