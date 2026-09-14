# Offline-first Android client with server sync

Status: phase 1 (the server) is implemented and merged. The client half is superseded: the
phone is a native Android app, designed in
`logb_mobile/docs/superpowers/specs/2026-09-14-logb-android-design.md`, not the Capacitor shell
below. The "prerequisite for phase 4" was resolved by having creates arrive through REST with
the row's final offline values and its own `client_uuid`, so no queued `set` predates the
create; the server additions that make this possible (`client_uuid` on creates, `client_uuid`
in responses, `entity_id` on pull rows, logged reference cleanups) shipped in the
`android-prereqs` branch.
See "What phase 1 actually built" at the end for how the implementation amended this design.

## Problem

logb today is a PWA whose offline story stops at opportunistic caching. Workbox keeps the
last 200 API GETs for a week and 300 file blobs for a month, so a screen the user happened to
visit recently still renders on a dead connection — and nothing else does. Writes fare better
but only just: `outbox.ts` queues three op kinds, all creates, and says why in its own header
comment — an edit or a delete "would have to be reconciled against whatever the server did in
the meantime; a create cannot disagree with anything".

That is the wrong shape for a phone. The goal is an Android client where the network is
irrelevant to every interaction: all data readable, full create/edit/delete, photos both
directions, for as long as the phone is offline.

## Scope

In scope: an Android application, built with Capacitor from the existing Svelte source, that
reads and writes a complete local SQLite mirror and reconciles with the server in the
background.

Out of scope: the desktop browser, which keeps today's behaviour unchanged — online-only
against the API, with the existing Workbox caching. Offline-first is an Android concern only.
The query layer is written against an interface the same plugin implements over wasm+OPFS, so
enabling the browser later is configuration rather than a rewrite, but it is not built now.

## Decisions

**Local truth.** The phone renders from local SQLite, always. Sync is a background reconciler,
never a fetch path on the way to a screen. This is the load-bearing decision: it makes offline
the default state rather than a degraded one, and removes optimistic-update-and-rollback from
the client entirely.

**Identity.** Every syncable table gains a `client_uuid` (nullable at the schema level, since
SQLite cannot `ALTER TABLE ADD COLUMN NOT NULL` without a default, with uniqueness enforced by
an index) — `objects`,
`activities`, `reminders`, `attachments`, `files`. The client mints it, so a row born offline
has an identity before the server has seen it, and an object created offline can carry
activities and attachments that reference it. The server keeps its integer `id` as the
primary key and maps UUID to id on arrival. Ops address entities by UUID only. `tempId` and
`persistResolvedId` in `outbox.ts` are deleted.

**Conflict resolution.** Field-level last-write-wins. Two devices editing different fields of
one activity both keep their edit; a true same-field collision resolves silently to the newer
write. No conflict UI, no conflict store, no user decision on a phone.

**Clocks.** Field-level LWW across devices means comparing timestamps that devices produced.
Every sync response carries the server's time; the client stores the offset and stamps
`edited_at` in corrected time. Ties break on `device_id` string comparison so every participant
resolves identically without coordination.

**Deletes.** Soft, via `deleted_at`, set by a delete op. Rows and their `changes` entries
survive 90 days, then a purge job in `tasks.rs` removes both. A phone whose cursor predates the
purge horizon is told to re-bootstrap.

**Photos.** Everything mirrors eventually. Thumbnails download eagerly and are never evicted,
so the mirror always renders. Full-size originals wait for an unmetered connection and become
an LRU cache under a user-set budget, defaulting to 4 GB — when storage binds, originals are
evicted and re-downloaded on demand while metadata and thumbnails stay intact.

## Server

Two new tables:

```sql
CREATE TABLE changes (
  seq          INTEGER PRIMARY KEY AUTOINCREMENT,  -- the pull cursor
  entity       TEXT NOT NULL,        -- object | activity | reminder | attachment | file
  entity_uuid  TEXT NOT NULL,
  op           TEXT NOT NULL,        -- create | set | delete
  field        TEXT,                 -- NULL for create and delete
  value        TEXT,                 -- JSON scalar
  edited_at    TEXT NOT NULL,        -- skew-corrected device time; the LWW key
  applied_at   TEXT NOT NULL,        -- server receive time
  user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  device_id    TEXT NOT NULL,
  client_op_id TEXT NOT NULL          -- idempotency; the outbox already mints these
);
CREATE INDEX idx_changes_user_seq ON changes(user_id, seq);
-- Scoped per user, not global: a globally unique op id lets one account's id collide with
-- another's, and the idempotency lookup then reports the stranger's op as accepted without
-- applying it -- a lost write reported as success.
CREATE UNIQUE INDEX idx_changes_user_op ON changes(user_id, client_op_id);

CREATE TABLE field_clock (
  entity      TEXT NOT NULL,
  entity_uuid TEXT NOT NULL,
  field       TEXT NOT NULL,
  edited_at   TEXT NOT NULL,
  device_id   TEXT NOT NULL,
  PRIMARY KEY (entity, entity_uuid, field)
);
```

An incoming op wins iff its `edited_at` is greater than the stored `field_clock.edited_at`, or
equal with a greater `device_id`. A losing op is **accepted, not rejected** — it is recorded and
reported back as `superseded`, and the client applies the winner locally. Only a malformed or
unauthorized op is rejected.

Three endpoints:

- `POST /api/sync/push` — a batch of ops. Returns per-op `accepted | superseded | rejected`
  plus UUID-to-id mappings for creates. Idempotent on `client_op_id`, so a push whose response
  was lost replays safely.
- `GET /api/sync/pull?since=<seq>&limit=` — ops after the cursor, plus `server_time`,
  `next_seq`, and a `complete` flag. Returns `410` when `since` predates the purge horizon.
- `GET /api/sync/bootstrap` — a full snapshot for a new device, or one that got a `410`.
  Cheaper than replaying an entire log.

Blobs continue to use the existing `/api/files/<id>` and upload routes unchanged.

## Client

Native SQLite through `@capacitor-community/sqlite`. The mirror holds the syncable tables keyed
on `client_uuid`, plus three local-only tables: `ops` (the pending queue), `sync_state` (cursor
`seq`, clock offset, `device_id`), and `blobs` (sha256, local path, presence, `last_access` for
eviction).

Every write is one transaction: apply to the mirror, append the op, commit. Remote ops apply
through the same LWW rule the server uses, so the client converges without needing to trust the
order in which it received them.

Modules: `lib/local/schema.ts` for local migrations, `lib/local/repo.ts` for queries returning
the exact shapes the API returns today (`MemObject`, `Activity`), `lib/sync/engine.ts` for the
push/pull/apply loop, `lib/sync/ops.ts` for recording and applying ops.

**The seam is the risk.** On Android this replaces `outbox.ts`, `idb.ts` and `object-cache.ts`
outright while desktop keeps them, so components must import from a `lib/data/` interface with
two implementations rather than calling `api()` directly. Introducing that seam touches every
component and lands as its own phase, with no behaviour change, before any Capacitor code
exists.

Sync runs on app foreground, on manual pull-to-refresh, and on a periodic Android background
task. Metadata syncs on any connection; blob transfer waits for unmetered.

## Authentication

A Capacitor app is a separate origin and cannot use the `logb_session` cookie, so it
authenticates with a `logb_pat_` bearer token — a path that already exists and is tested
(`auth.rs:182`, `LOGB_CORS_ORIGINS`).

The user logs in online once; the app mints a PAT via `/api/auth/tokens` and stores it in
Capacitor secure storage backed by the Android Keystore, requiring biometric or PIN to decrypt.
App open unlocks, decrypts, then syncs. Offline unlock works because the keystore is local, so
the mirror stays readable and ops keep queuing with no connection.

On a 401 the app discards the PAT and requires online re-login, but keeps the mirror. Mirrors
are per user, so a shared household device holds one per account.

## Testing

The server side tests at the HTTP level like everything else in `tests/`: LWW resolution as
table-driven cases covering newer-wins, older-superseded, and the `device_id` tie-break; push
idempotency on a replayed `client_op_id`; and the purge-horizon to `410` to bootstrap path
explicitly.

The sync engine is pure logic over a store interface — the pattern `outbox.ts` already
established with `memoryStore()` — so it tests against in-memory SQLite with no device.
Emulator-driven end-to-end coverage of the Capacitor build is a disproportionate lift; it gets a
manual smoke checklist instead, and that is a deliberate gap rather than an oversight.

## Phases

Each phase is its own plan and build cycle.

1. **Sync protocol backend.** Schema, `changes` and `field_clock`, the three endpoints, LWW,
   tombstones, purge job. Server-only, fully testable, ships without touching any client.
   Independently useful and carries no client risk.
2. **Data-source seam.** Introduce `lib/data/`; desktop keeps the API implementation. Pure
   refactor, no behaviour change.
3. **Capacitor shell and mirror, reads only.** App renders from local, pull-only sync, writes
   still go online.
4. **Offline writes.** Op recording, push, supersede handling.
5. **Blobs.** Download queue, WiFi gating, LRU budget, offline capture and upload.
6. **Unlock and keystore.**

## Known interaction

Automated backup is a separate, unstarted concern, but it collides with this one: restoring a
server snapshot silently rolls back writes that phones believe were accepted. Whichever lands
second must account for the other — most likely by having a restored server advertise a fresh
bootstrap epoch that forces clients to reconcile rather than resume from a stale cursor.

## What phase 1 actually built, and what it constrains

Phase 1 is merged. Two things about it changed the design rather than merely implementing it,
and both bind the phases that follow.

**REST writes are logged too.** The design as written only ever had the sync endpoints writing
to `changes` and `field_clock`, which left the browser invisible to the protocol: a device would
never learn of an edit or delete made in the browser, and — worse — a REST write left the field
clock holding whatever older timestamp a phone last wrote, so a sync op stamped *earlier* than
the browser edit still won and overwrote it. Every REST create, update and delete now records
per-field ops and stamps the clock, through shared helpers in `src/sync/record.rs`. The delete
cascade is shared code rather than two matching copies, because the two paths drifted twice
during phase 1 and each drift was caught only in review.

### Prerequisite for phase 4: a create supersedes the edits made before it

`record_create` stamps `field_clock` for every whitelisted field at creation time. That is
deliberate — an unstamped field loses to nothing, so a stale offline edit would otherwise win
against a fresh row by default. The consequence is a rule the client must be built around:

> Any sync `set` whose `edited_at` predates the server-side create is superseded, for every
> field.

So a client that flushes an offline outbox as "REST-create the row, then push the `set` ops
queued against it" loses all of those ops. Phase 4 has to pick one of:

- give the create path the row's true offline creation time, so the edits that followed it are
  genuinely later; or
- carry offline creates as `create` ops through sync rather than through REST, so the whole
  sequence shares one clock.

The choice is open. What is not open is ignoring it — the failure is silent, and it presents as
"some of my offline edits vanished".

### Smaller constraints inherited from phase 1

- `changes.value` is **double-encoded** JSON on the wire: a string value arrives as
  `"\"Golf VII\""`, and a client must parse it. An explicit `value: null` on a `set` is stored
  as SQL NULL and is therefore indistinguishable from an absent value.
- `GET /sync/bootstrap` is the only path that exposes `client_uuid`, and ops address entities by
  UUID alone, so a client cannot push anything until it has bootstrapped.
- Two reference cleanups — clearing an object's cover when its attachment goes, and unlinking
  `reminders.done_activity_id` — change rows without logging a `set` or stamping the clock. A
  pulling device does not learn of either. Worth closing before phase 3 relies on the feed being
  complete.
- `client_uuid` is nullable at the schema level and backfilled rows carry a 32-character hex
  value while newly minted ones are 36-character hyphenated v4. Nothing may validate it as a
  strict UUID shape.

## Later additions

### 2026-09-14: own object types

Users can define their own object types (`docs/superpowers/specs/2026-09-14-own-types-design.md`).
What that changes on the wire:

- **Snapshot.** `GET /sync/bootstrap` gains an `object_types` array: the user's non-deleted rows
  of `object_types`, shipped verbatim like the other tables (`categories` is JSON text).
- **Object types.** `objects.type` is a built-in key or `custom:<uuid>`, where the uuid is an
  `object_types.client_uuid`. The key never changes when the type is renamed.
- **Entity.** Ops and pulled changes may name the entity `object_type`. Settable fields are
  `name`, `icon`, `categories` (JSON text, stored normalised) and `counter_unit`. A `set` is
  checked against the whole type (the stored row plus the changed field), and so is a rename
  against the user's other type names. A `delete` is rejected with reason "in use by N object(s)"
  while any live object uses the type.
- **Create is real for types.** Other entities' `create` ops only announce a row made over REST.
  An `object_type` create inserts the row: `entity_uuid` is the new type's lower-case
  `client_uuid`, and `value` carries `{name, icon, categories, counter_unit}`. Ops in one push
  apply in order in one transaction, so a later op in the same push (an object's `set type`) can
  use the type. Replaying the create for the caller's own live type is `accepted` and inserts
  nothing. A uuid already used by anyone else, or by a deleted type, is rejected. As with every
  create op, the logged change carries no value, so a pulling device reads the row through
  bootstrap or `GET /types`, using `entity_id`.
- **REST writes are logged.** `POST`/`PATCH`/`DELETE /types` record create/set/delete changes and
  stamp field clocks, like every other REST write.

Compatibility rule for clients: **ignore unknown snapshot keys and unknown entities, and show an
unknown type key as "other".** No epoch rotation was needed. No client consumes the feed yet (the
PWA has no sync client), so nothing could fail on the new entity.
