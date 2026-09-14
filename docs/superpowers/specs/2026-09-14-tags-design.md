# Tags on objects and entries

Status: implemented. Second of three projects (objects list → tags → own types).

Objects and entries can only be told apart by name, type and category. A household wants its own
words for them -- "lease", "Garage 2", "warranty", "tax 2026" -- and wants to find everything
carrying one.

## What a tag is

- Free text. Stored as typed after normalising: trimmed, runs of whitespace collapsed to one space.
- At most 32 characters, at most 10 tags on one object or entry. More is a 400 naming the limit.
- Two tags are the same when they are equal ignoring case and accents; within one item the first
  spelling is kept and later duplicates are dropped. Across items, the suggestions show the
  spelling used most often.
- Tags are per user: nobody sees or is suggested another user's tags.

## Colours

A tag's colour comes from its name, so the same tag has the same colour everywhere and on every
device, with nothing to configure. The folded name (lower case, accents removed) is hashed
(FNV-1a, 32-bit) and picks one of 8 palette entries.

Each entry is a background and a text colour, defined as CSS tokens for the light theme and again
for the dark theme (`--tag-0-bg`, `--tag-0-fg`, …). Every pair meets WCAG AA for normal text
(contrast ratio ≥ 4.5:1), checked by a unit test that computes the ratio from the token values, so
a palette edit that makes a chip hard to read fails the build.

## Editing

Object form and entry form get a **Tags** field: chips for the current tags, each with a remove
button, and a text input. Enter or comma adds the typed tag; Backspace in an empty input removes
the last chip. While typing, up to 8 existing tags that start with (then contain) the text are
suggested, from `GET /tags`. A tag that would exceed the limits is refused in place with a sentence
saying why.

## Display and filtering

- **Objects list:** chips on each card, after the info line. Tapping a chip filters the list to
  objects carrying that tag; the active tag filter shows above the list as a removable chip and
  combines with the search box, tabs and sort from the objects list project. The search box also
  matches tags.
- **Object Info tab:** chips under the type line.
- **Timeline:** chips on each entry. Tapping one filters the timeline to entries with that tag,
  shown and cleared like the existing category filter; it combines with the category filter.
- **Search screen:** object and entry hits also match on tags.

A chip is a button with the tag as its accessible name; the colour is decoration, never the only
signal.

## Server

- Migration (SQLite and PostgreSQL): `objects.tags` and `activities.tags`, `TEXT NOT NULL DEFAULT
  '[]'`, a JSON array of strings.
- Object and activity inputs accept `tags: string[]` (absent keeps the current tags on update,
  like other optional fields); outputs always carry `tags`.
- Normalisation and limits live in one pure function in `src/domain/tags.rs`, used by REST,
  sync writes and import alike.
- `GET /tags`: the caller's distinct tags across non-deleted objects and entries, each with its
  most-used spelling and a count, sorted by count then name.
- `GET /objects/{id}/activities?tag=…`: entries carrying the tag (matched case- and
  accent-insensitively after loading candidates with a portable `LIKE` on the JSON text).
- Search: object and activity queries also match the `tags` text.
- Sync: `tags` joins the synced fields of objects and activities (`sync::mod` field lists), so
  pull, push, field clocks and offline edits carry it like `notes`. Concurrent edits to one item's
  tags resolve like any other field: the later edit wins for the whole list.
- Export/import: `tags` on objects and activities; archives without it import with no tags.
- `docs/openapi.json`: the field, `GET /tags`, the `tag` parameter.

## Tests

- Rust unit: normalisation, folding equality, limits, duplicate dropping.
- Rust integration (SQLite and PostgreSQL): create/update/read tags on both; limits give 400;
  `GET /tags` counts and spelling, per user; `?tag=` filtering ignoring case and accents; search
  matches tags; sync pull carries tags and a pushed edit updates them; export/import round trip and
  an old archive without tags.
- Vitest: colour index is stable and spread; every palette pair ≥ 4.5:1 in both themes; chip-input
  logic (add, dedupe, limits, backspace removal, suggestions).
- Playwright: tag an object and an entry via the forms with a suggestion; chips appear on the card,
  Info tab and timeline; tapping filters the objects list and the timeline; tags survive a reload.

## Out of scope

Renaming or merging tags across items, tag colours chosen by hand, tags on reminders.
