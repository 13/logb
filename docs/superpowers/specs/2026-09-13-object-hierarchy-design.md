# Objects that contain other objects

Status: implemented.

## Problem

A house is not one thing to maintain — it is a container for many. "When did I last change
the bulb in the garage's main light?" cannot be answered today because there is no way to
represent "the garage" as a place with its own things in it, only as one flat object called
"House" with no structure underneath.

`objects` has no relationship to itself. Every object is a peer of every other, at the top
level, forever.

## Approach

An object gains an optional parent, pointing at another object: `objects.parent_id`,
self-referential, nullable. A house is an object; a garage is an object whose parent is the
house; "Main light" and "Secondary light" are two objects whose parent is the garage. Each
keeps its own name and its own independent activity history — two lights in one room are
distinguished the same way two cars are, by name.

Nothing else changes about what an object is. It still has a type, a counter, activities,
attachments, reminders. A room or a fixture is not a new kind of thing; it is an object placed
inside another one.

No depth limit is enforced. The data model does not care whether it is two tiers or five; the
UI presents it as a breadcrumb and a contents list regardless of how deep it goes.

## Why this is the riskiest feature built on this codebase so far

Two properties of the existing system, both true for reasons that predate this feature, turn a
simple-looking column into the most delicate part of the sync layer:

**Deletion is a soft tombstone, not a real `DELETE`.** `ON DELETE CASCADE` only fires for an
actual `DELETE`, so it does nothing when a user deletes an object through the app — that write
is `UPDATE objects SET deleted_at = ...`. `record::cascade_object` exists because of exactly
this: deleting an object has to explicitly tombstone its activities, attachments and reminders,
in the one function shared by the REST handler and `sync::apply::apply_op`'s delete arm, so the
cascade cannot drift into two different behaviours depending on which door a client came
through.

`parent_id` adds a fourth thing that cascade has to reach, and it is not a flat list like the
other three: deleting "Garage" must tombstone "Main light" and "Secondary light", and if either
of those itself has children, theirs too. `cascade_object` becomes recursive — after tombstoning
an object's direct children, it runs itself again for each child just tombstoned. This
terminates because cycles are refused at write time (below), so the tree a delete walks is
always finite.

**A field that references another row needs two doors guarded identically, not one.**
`cover_attachment_id` already established the pattern, and the comment on it says why plainly:
*"the REST handlers already refuse a cross-object reference; sync has to refuse it identically,
or it is just a second, unguarded door onto the same write."* `parent_id` needs the same
guard, sharing one implementation between the REST handler (`src/api/objects.rs`) and sync's
`Set` handling (`src/sync/apply.rs`), refusing:

- a parent that does not exist, or belongs to another user, or is already deleted;
- an object naming itself as its own parent;
- **a parent that is a descendant of the object being updated** — the one failure mode nothing
  in this app has had to guard against before, because nothing before this pointed an object at
  another object of the same kind. Reparenting "House" underneath "Main light", when "Main
  light" is already inside "House", would corrupt the tree into a cycle with no bottom.

The cycle check walks the candidate parent's own ancestor chain (parent, parent's parent, and
so on) and refuses if the object being updated appears in it, or if the candidate is the object
itself. `WITH RECURSIVE`, portable across SQLite and PostgreSQL, since this project runs on
either.

`parent_id` is added to `Entity::Object`'s field whitelist as `Integer`
(`src/sync/mod.rs`), the same shape as `cover_attachment_id`. A field missing from that
whitelist binds `NULL`, reports success to the client, and advances the field clock — this
project has shipped that exact defect twice. It is the first thing any review of this feature
checks, not the last.

## What a user sees

**The object form** gains a parent picker: search over the user's own, non-deleted objects,
excluding the object itself and its own descendants — refused at the UI layer for the same
reason it is refused at the API layer, so a doomed choice is never offered rather than merely
rejected after the fact.

**The object detail page** gains a breadcrumb ("House › Garage") when the object has a parent,
and a "Contents" section listing direct children, each showing its type icon and what its own
detail page already shows on a dashboard row — with an obvious way to add a new object here,
pre-filling this object as the parent.

**The dashboard** shows only root objects — those with no parent. A house with ten fixtures
does not flood the top-level list; its contents live on its own page. `GET /objects` gains
`archived` as it already has, and now also excludes anything with a `parent_id` unless a new
`parent_id` query parameter asks for exactly that parent's children.

**Search** still searches every object regardless of nesting, and a result with a parent shows
it ("Main light — in Garage") so a match out of context is not a mystery.

## Export, import, and the copy command

`/export` is scoped to one object and its own activities, attachments and reminders — it does
not carry other objects, so a parent living outside the exported object is never in the
archive to begin with. Carrying a raw `parent_id` across an export would point at nothing on
the far side, or, worse, at whatever unrelated row happens to hold that id after import
reassigns every object a fresh one.

The rule: **`parent_id` is not written to `data.json`, and an imported object always arrives
with no parent**, regardless of what it had. This matches this project's existing standard for
what export claims — it is a portable copy of one object's own history, not a graph of its
relationships to others. Re-parenting an imported object is a manual step afterwards, the same
one used to build the tree in the first place.

`logb --copy-to` is unaffected and needs no change: it preserves every id verbatim rather than
reassigning them, so `parent_id` copies across correctly for free, the same way
`cover_attachment_id` already does.

## What this deliberately does not do

**No new object types.** "House", "Garage" and "Main light" are all just objects, picked from
the nine types that already exist — most naturally `home` for the house, whatever fits for the
rest. Introducing a `room` or `fixture` type is a separate, smaller decision for later; nothing
here needs it, and the type system already has a per-type activity vocabulary that would need
its own design pass if a type were added.

**No rollup.** A house's total cost and activity count remain the house's own — they do not
sum its children's. "How much have I spent on this house across everything in it" is a real
question and a reasonable future feature, but it is a different one from "when did I last
change this light", which is what was asked for.

**No new activity categories.** "Changed the bulb" is `maintenance` or `repair`, both of which
already exist and apply to every type.

## Testing

- The cycle guard is exercised directly against the shared check, not only through the API:
  a three-level chain (House → Garage → Light), attempting to reparent House under Light, must
  be refused by both the REST path and a sync `set` op naming the same reparenting — proving
  the two doors agree rather than merely asserting each in isolation.
- Deleting an object with a two-level-deep tree beneath it must tombstone every descendant, and
  each descendant's own activities, attachments and reminders — proven by seeding a real tree
  through the API, deleting the root, and asserting every row underneath carries a tombstone.
- The whitelist-versus-schema test that already exists must continue to pass with `parent_id`
  in it — a missing entry there fails that test by design.
- Sync test: a `set object.parent_id` op pointing at another user's object, a deleted object,
  and the object's own descendant, must each be `Rejected` with a reason, not a 500 and not a
  silent success.
- A dashboard fetch with no seeded hierarchy must be unaffected — this is the regression that
  matters most, since every existing installation's objects all have `parent_id IS NULL` and
  must keep appearing exactly as they do today.
- Exporting an object with a parent, then importing that archive into the same or a different
  account, must produce an object with no parent — proving the drop is deliberate rather than
  an accidental null from a column that does not yet exist on the far side.
