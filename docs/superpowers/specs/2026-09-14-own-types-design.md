# Own object types

Status: approved, not implemented. Third of three projects (objects list → tags → own types).

The nine built-in types cover common things, but a household owns more kinds: an e-scooter, a
boat, a heat pump, a camera. Filing those under "Other" loses the icon, offers every entry
category, and lumps them together in statistics.

## What a custom type is

Each user can create their own types. A type has:

- **Name**: 1–40 characters after trimming, unique among that user's non-deleted types ignoring
  case and accents.
- **Icon**: one of the app's existing icons (the `IconName` set).
- **Entry categories**: a non-empty subset of the existing activity categories; `other` is always
  included.
- **Default counter unit**: none, `km`, `mi` or `h`. It pre-fills a new object's unit; it never
  changes an existing object.

Types are per user: nobody sees another user's types. The nine built-in types stay, unchanged,
and cannot be edited or deleted. Reminder templates remain built-in types only.

## Where they appear

- **Settings > Types**: built-in types listed read-only; own types with add, edit and delete.
  Deleting a type still used by any non-deleted object is refused with the count ("Used by 3
  objects -- change their type first"), so nothing is re-filed silently.
- **Object form**: the type picker lists built-in types, then an optgroup "Your types". Choosing
  one of them sets the counter unit when the object has none yet.
- **Everywhere a type is shown or used** -- cards, object detail, search hits, timeline, entry
  form categories, statistics "By type": the custom type's name and icon, and its categories.
  One frontend lookup resolves any type key (built-in or custom) to label, icon and categories.
- **Offline**: the type list is cached like objects, so a device without a connection still
  names and draws its types.

## Data

- `object_types`: `id`, `user_id`, `client_uuid` (unique), `name`, `icon`, `categories` (JSON
  array text), `counter_unit` (nullable, `km|mi|h`), `created_at`, `updated_at`, `deleted_at`.
- `objects.type` holds a built-in key (`car`) or `custom:<client_uuid>`. The `CHECK` constraint
  on `objects.type` is removed (SQLite: table rebuild in the style of `0009_object_types.sql`;
  PostgreSQL: drop the constraint). Validity moves to one server function used by REST, sync
  apply and import: a built-in key, or `custom:` plus the uuid of the caller's own non-deleted
  type.
- The uuid (not the integer id) is the reference, so a type and objects using it can be created
  offline in one go and replay in order.

## API

- `GET /types` -- the caller's non-deleted custom types.
- `POST /types`, `PATCH /types/{id}`, `DELETE /types/{id}` -- with the rules above; delete returns
  409 with the in-use count when objects still use it. `client_uuid` accepted on create and
  replayed idempotently, as objects do.
- `docs/openapi.json` describes all of it; `ObjectType` in the schema becomes a string that is a
  built-in key or `custom:<uuid>`.

## Sync, export, copy

- Sync gains an `object_type` entity (create/set/delete; fields `name`, `icon`, `categories`,
  `counter_unit`) with field clocks like other entities, and the pull feed carries type rows.
  Before implementation the plan checks how an existing mirror client treats an unknown entity
  in the feed; if it would fail, the change rotates the sync epoch so every device re-syncs once.
- Export includes each user's types; import creates them before objects and accepts archives
  without types.
- `copy::TABLES` and the schema-parity test include `object_types`.

## Statistics

`by_type` buckets stay the object's `type` value; the Statistics screen labels `custom:<uuid>`
buckets with the type's name (or "Deleted type" if it no longer exists).

## Tests

- Rust (SQLite and PostgreSQL): migration keeps existing types and accepts `custom:` values;
  type CRUD, name uniqueness, category and unit validation; delete refused while in use; an object
  with an own type; another user's type rejected on objects; sync create/set/delete round trip and
  an object referencing a type created in the same push; export/import round trip and an old
  archive; statistics by type.
- Vitest: type lookup (built-in, custom, unknown), categories for custom types.
- Playwright: create "E-scooter" (icon, km, categories) in Settings; create an object of that type
  and see name and icon on its card; its entry form offers only its categories; statistics show
  it; deleting it is refused while in use.

## Out of scope

Editing or hiding built-in types, reminder templates for custom types, shared types between users,
custom fields per type.
