/**
 * The offline outbox's API surface: queued writes, the flush pass and the queue reads the UI
 * shows. Split from `api.ts`, which keeps the plain transport (`api`, `apiPage`, `upload`), the
 * stale-response tracking and the session hooks. Everything here is re-exported from `api.ts`,
 * so callers import from there as before.
 */
import { cancelQueuedActivityOps, cancelQueuedObjectOps, createKeyedLock, createLock, enqueue, isQueuedActivity, isQueuedObject, isQueuedUnderActivity, isQueuedUnderObject, mayLeaveDevice, newOpId, pendingCount, removeQueuedActivity, replay, serialize, SkipOp, updateQueuedActivityBody, updateQueuedObjectBody, type OutboxStore, type QueuedOp } from './outbox';
import { idbStore } from './idb';
import { ApiError, isRejection, isUnauthenticated } from './api-error';
import { api, apiWithStatus, editedAtNow, isOurs, outboxUser, SAVE_TIMEOUT_MS, sendGateOpen, upload } from './api';

let store: OutboxStore = idbStore();

/**
 * Test seam only: swaps the store `createQueued`/`flushOutbox` write through, so a unit test
 * can hand them a `memoryStore()` (see `./outbox.ts`) instead of the real IndexedDB-backed one,
 * which does not exist in the test environment. Production never calls this -- the module
 * always starts, and stays, on the real `idbStore()` above.
 */
export function setOutboxStoreForTesting(s: OutboxStore): void {
  store = s;
}

/**
 * POST that survives a dead connection: on anything that isn't a genuine server rejection
 * (see `isRejection` in `./api-error.ts`) the op is queued and replayed later. `tempId` is the
 * placeholder the caller shows in the meantime. The `client_op_id` is generated once here, not
 * per attempt, so every retry of this op — including the one sent later by `replay` — carries
 * the same id and a lost response can never duplicate the row.
 */
export async function createQueued<T>(path: string, body: Record<string, unknown>, tempId?: number): Promise<T | null> {
  const id = newOpId();
  // Read the owner BEFORE the request, not in the catch below. A 401 runs the unauthorized
  // handler on its way out, which sets the current user to null (see ../stores/session.ts), so
  // by the time the catch enqueues there is nobody signed in to attribute the write to -- and
  // the write would land in the queue untagged, free for the next person on the device to
  // claim. The op belongs to whoever was signed in when the user made it.
  const userId = outboxUser() ?? undefined;
  const leaves = mayLeaveDevice();
  try {
    // A timeout lands in the catch below like a dropped connection and queues the op. The
    // request may still have reached the server; the replay carries the same `client_op_id`, and
    // the server answers a repeat with the row the first one made (src/api/activities/write.rs)
    // -- that row as it was, so the op records that it may exist (`maybeLanded`) for a later
    // edit or cancel of the draft to reach it (see `doFlushOutbox`).
    return await api<T>('POST', path, { ...body, client_op_id: id }, SAVE_TIMEOUT_MS);
  } catch (e) {
    // A 401 is a rejection the user can undo by logging back in, so the write is queued rather
    // than thrown away -- the same reasoning `replay` applies to an op that meets an expired
    // session mid-pass (see `isUnauthenticated` in ./api-error.ts). Without this, a session
    // that lapsed while the form was open discarded everything typed into it: the unauthorized
    // handler navigates to /login, and the entry existed nowhere else.
    if (isRejection(e) && !isUnauthenticated(e)) throw e;
    try {
      await enqueue(store, { id, kind: 'activity.create', path, body, tempId, attempts: 0, userId, ...landedIf(leaves, e) });
    } catch {
      // The write reached neither the server nor the local queue: nothing durable remembers
      // it any more, so the caller must be told rather than navigating away as though the
      // entry were saved. This is the one failure mode `isRejection`'s "just queue it, the
      // client_op_id makes replay safe" reasoning does not cover.
      throw new Error('outbox.queue-failed');
    }
    return null;
  }
}

/** A failed create may have landed unless the device had no network when it was sent, or the
 *  server answered it with a refusal (a 401, queued all the same). See `QueuedOp.maybeLanded`. */
function landedIf(leaves: boolean, e: unknown): { maybeLanded?: true } {
  return leaves && !isRejection(e) ? { maybeLanded: true } : {};
}

/** Creates an object durably while offline. Objects use `client_uuid` for idempotency (activity
 * creates use `client_op_id`), so this has its own small wrapper and replay kind. */
export async function createObjectQueued<T>(body: Record<string, unknown>, tempId: number): Promise<T | null> {
  const id = newOpId();
  const userId = outboxUser() ?? undefined;
  const leaves = mayLeaveDevice();
  try {
    // Timeout: see createQueued. A repeated `client_uuid` answers with the first attempt's row.
    return await api<T>('POST', '/objects', { ...body, client_uuid: id }, SAVE_TIMEOUT_MS);
  } catch (e) {
    if (isRejection(e) && !isUnauthenticated(e)) throw e;
    try {
      await enqueue(store, { id, kind: 'object.create', path: '/objects', body, tempId, attempts: 0, userId, ...landedIf(leaves, e) });
    } catch {
      throw new Error('outbox.queue-failed');
    }
    return null;
  }
}

export async function createReminderQueued<T>(path: string, body: Record<string, unknown>): Promise<T | null> {
  const id = newOpId();
  const userId = outboxUser() ?? undefined;
  // Timeout: see createQueued. A repeated `client_uuid` answers with the first attempt's row.
  try { return await api<T>('POST', path, { ...body, client_uuid: id }, SAVE_TIMEOUT_MS); }
  catch (e) {
    if (isRejection(e) && !isUnauthenticated(e)) throw e;
    try {
      await enqueue(store, { id, kind: 'reminder.create', path, body, attempts: 0, userId });
    } catch {
      // Same as its siblings: a raw IndexedDB error ("quota") told the user nothing useful.
      throw new Error('outbox.queue-failed');
    }
    return null;
  }
}

/**
 * PATCH that survives a dead connection, for an edit to a row the server already has. Resolves
 * `true` when the server took it, `false` when it only reached the queue.
 *
 * The queued body carries `edited_at` -- the moment the person made the edit, not the moment
 * it is finally sent -- so the server can keep a newer change someone else made in the meantime
 * instead of overwriting it with an older one (see `edited_at` on `ActivityInput`,
 * src/api/activities.rs). The first attempt carries the same stamp: it may time out and still
 * land, later than the user's next edit even, and unstamped it would count as made the moment
 * it arrived and overwrite that newer edit. `notBefore` is a row's `created_at`, for an edit to
 * a row that may have been made only just now (see `foldStamp`).
 */
export async function updateQueued(path: string, body: Record<string, unknown>, notBefore?: string): Promise<boolean> {
  const id = newOpId();
  const editedAt = foldStamp(editedAtNow(), notBefore);
  // Before the request, for the same reason as in `createQueued`.
  const userId = outboxUser() ?? undefined;
  try {
    // Timeout: see createQueued. If the timed-out PATCH lands after all, it carries the same
    // `edited_at` as its replay, so whichever arrives second changes nothing.
    await api('PATCH', path, { ...body, edited_at: editedAt }, SAVE_TIMEOUT_MS);
    return true;
  } catch (e) {
    if (isRejection(e) && !isUnauthenticated(e)) throw e;
    try {
      await enqueue(store, { id, kind: 'activity.update', path, body: { ...body, edited_at: editedAt }, attempts: 0, userId });
    } catch {
      throw new Error('outbox.queue-failed');
    }
    return false;
  }
}

/**
 * Upload that survives a dead connection: the `attachment.upload` counterpart to `createQueued`
 * above. Sends multipart form data instead of JSON, and follows `createQueued` exactly
 * otherwise -- the `client_op_id` is minted once here and reused for the immediate attempt and
 * every later replay, and only a genuine server rejection (`isRejection`) is rethrown rather
 * than queued.
 *
 * `activityId` may be a real id or a negative temp id -- offline, the parent activity may
 * itself still be sitting in the outbox with no real id yet (see `tempId` in `./outbox.ts`).
 * A temp id means the server has never heard of that activity, so there is nothing to "try
 * first": sending now would 404 rather than queue, so that case skips straight to enqueuing.
 * `replay` rewrites the temp id in the stored op to the real one once (or after) the parent
 * `activity.create` lands.
 */
export async function uploadQueued<T>(path: string, file: Blob, filename: string, activityId?: number): Promise<T | null> {
  const id = newOpId();
  const userId = outboxUser() ?? undefined; // see createQueued: read before the request, not after

  // A pass in this tab may have landed the parent already without the form having heard yet
  // (see `landedDrafts`); the temp id's children were rewritten then, so this one must be too.
  if (activityId !== undefined && activityId < 0) activityId = landedDrafts.get(activityId)?.id ?? activityId;
  const body: Record<string, unknown> = activityId === undefined ? {} : { activity_id: activityId };
  const activityIsReal = activityId === undefined || activityId >= 0;
  if (activityIsReal) {
    try {
      const form = new FormData();
      form.append('file', file, filename);
      form.append('client_op_id', id);
      if (activityId !== undefined) form.append('activity_id', String(activityId));
      return await upload<T>(path, form);
    } catch (e) {
      if (isRejection(e) && !isUnauthenticated(e)) throw e;
      // Fall through to queue, same as createQueued -- including on a 401, for the same reason.
    }
  }
  try {
    // Under the store lock, and checked again in there: a pass landing the parent between the
    // check above and this write has either rewritten the queued upload by now, or not yet
    // taken the lock for that -- and then `landedDrafts` already has it (see `sendCreate`).
    await outboxLock.run(async () => {
      const real = typeof body.activity_id === 'number' ? landedDrafts.get(body.activity_id)?.id : undefined;
      await enqueue(store, { id, kind: 'attachment.upload', path, body: real === undefined ? body : { activity_id: real }, blob: file, filename, attempts: 0, userId });
    });
  } catch {
    // See the matching comment in createQueued: neither the server nor the local queue has
    // this write, so the caller must be told rather than treating the file as saved.
    throw new Error('outbox.queue-failed');
  }
  return null;
}

/** Notified after each `flushOutbox()` pass completes, so a mounted view can drop a synthetic
 *  pending entry the instant its real row lands instead of showing it until the next remount.
 *  Carries this pass's temp-id -> real-id resolutions (see `replay` in ./outbox.ts) so a view
 *  that minted a temp id itself (`ActivityForm.svelte`) can learn its own draft resolved
 *  without re-deriving it from the store -- an empty map on a pass that resolved nothing.
 *
 *  `changed` says whether the pass altered the queue at all (anything sent and removed, or
 *  parked dead). Passes run on every `visibilitychange`, overwhelmingly over an empty queue, so
 *  a view that reloads unconditionally re-fetches for nothing every time the tab regains focus.
 *  This is deliberately a property of the PASS rather than something each view works out by
 *  diffing the queue itself: a view's own snapshot is only ever as fresh as its last load, so
 *  it misses anything queued while it sat there (a photo attached from the Documents tab, say)
 *  and then skips the reload when that write finally lands. */
const flushListeners = new Set<(resolved: Map<number, number>, changed: boolean) => void>();
export function onOutboxFlushed(fn: (resolved: Map<number, number>, changed: boolean) => void): () => void {
  flushListeners.add(fn);
  return () => flushListeners.delete(fn);
}

/**
 * Asks the browser to keep this origin's storage rather than evicting it under pressure.
 *
 * The outbox is IndexedDB, and a queued upload carries the photo itself. By default that is
 * "best-effort" storage, which Android Chrome may clear when the device is low on space -- so
 * the one thing here that cannot be re-fetched from the server, because the server has never
 * seen it, is the thing most at risk. A granted request makes the origin's data persistent
 * until the user clears it themselves.
 *
 * Called once the session is known, since that is when there is something worth keeping and
 * when a browser that weights the decision by engagement is most likely to say yes. Failure is
 * not worth reporting: it is an optimisation, the API is absent on some browsers, and nothing
 * the user could do about it would help.
 */
export async function persistStorage(): Promise<boolean> {
  try {
    const storage = globalThis.navigator?.storage;
    if (!storage?.persist || !storage.persisted) return false;
    return (await storage.persisted()) || (await storage.persist());
  } catch {
    return false;
  }
}

/** The queue as one comparable value: which ops exist, whether each is parked, and how many
 *  attempts it has behind it. `attempts` is in there because a pass can SEND an op and then
 *  fail to remove it (an IndexedDB error in the write-back): the op survives, un-parked, so
 *  ids and dead flags alone look untouched even though the server now holds the write. */
async function queueSnapshot(): Promise<string> {
  return (await store.all()).map((o) => `${o.id}:${o.dead ? 1 : 0}:${o.attempts}`).join(',');
}

/**
 * Shared with `updateQueuedActivity` and `cancelQueuedActivity` below (see `createLock` in
 * `./outbox.ts`) so a replay pass and a UI write against the SAME queued op can never
 * interleave: whichever gets there first runs to completion -- read, act, write back -- before
 * the other is allowed to touch it at all. IMPORTANT 1: without this, a Save or Cancel fired
 * while a pass's `send()` is in flight raced the pass's own eventual write-back and lost -- the
 * pass, snapshotting the op before the UI write happened, would win by writing back exactly
 * what the user had just changed or removed, moments after telling them it worked.
 *
 * Two locks, not one (see `ReplayLocks` in `./outbox.ts`): `outboxLock` for each short
 * read-act-write of the store, and `opLocks` per op, held across that op's send. The pass used
 * to hold a single lock from its first send to its last, so a Save waited behind every
 * full-size photo upload in the queue -- and returning from the camera starts exactly such a
 * pass. Now a UI write waits only while the pass is sending the very op it touches.
 *
 * Named, so the locks span TABS as well as callers: the store they guard is IndexedDB, which
 * every same-origin tab shares, so a second tab replaying the same queue reproduces the same
 * interleaving between tabs that these close within one. On a browser with no Web Locks API the
 * names are inert and the exclusion stays tab-local, as it was before (see `createLock`).
 */
const outboxLock = createLock('logb-outbox');
const opLocks = createKeyedLock('logb-outbox-op');

/**
 * Runs a UI write to queued ops holding the lock of every op it `touches`, then the store lock
 * -- the same order `replay` takes them in. The ops are found first and locked second, so an op
 * that starts matching in between (queued meanwhile) sends the whole thing round again rather
 * than being written unlocked.
 */
async function underOpLocks<T>(touches: (op: QueuedOp) => boolean, fn: () => Promise<T>): Promise<T> {
  for (;;) {
    const ids = (await outboxLock.run(() => store.all())).filter(touches).map((o) => o.id).sort();
    type Outcome = { done: true; value: T } | { done: false };
    const locked = ids.reduceRight<() => Promise<Outcome>>(
      (inner, id) => () => opLocks.run(id, inner),
      () => outboxLock.run(async (): Promise<Outcome> => {
        if ((await store.all()).some((o) => touches(o) && !ids.includes(o.id))) return { done: false };
        return { done: true, value: await fn() };
      }),
    );
    const outcome = await locked();
    if (outcome.done) return outcome.value;
  }
}

/**
 * Temp id -> real row for every draft create this tab's passes have landed. A pass reports its
 * resolutions to `onOutboxFlushed` only when it ends, which can be a long upload later; a Save
 * in the meantime still names the draft by its temp id, finds no queued create left to fold
 * into, and looks the real row up here instead (see `updateQueuedActivity`). So do a photo
 * picked meanwhile (`uploadQueued`) and a Cancel (`cancelQueuedActivity`).
 */
type CreatedRow = { id: number; created_at?: string };
const landedDrafts = new Map<number, CreatedRow>();

/**
 * Replays a draft's create, then whatever became of the draft since the create was first sent.
 * That send may have landed (`QueuedOp.maybeLanded`), and the replay's repeated idempotency key
 * is then answered with the row it made, as it was -- 200 rather than 201 (see `apiWithStatus`).
 * So an edit folded into the queued body afterwards (`foldedAt`) goes to that row as a PATCH,
 * and a cancelled draft (`cancelled`) is created only to learn its id and then deleted. Every
 * step is safe to repeat, so a failure part-way through just leaves the op to be retried whole.
 */
async function sendCreate(
  op: QueuedOp,
  body: Record<string, unknown>,
  rowPath: (id: number) => string,
  editBody: (row: CreatedRow) => Record<string, unknown>,
): Promise<CreatedRow | null> {
  let answer: { status: number; body: CreatedRow };
  try {
    answer = await apiWithStatus<CreatedRow>('POST', op.path, body, SAVE_TIMEOUT_MS);
  } catch (e) {
    // Refused (a 409 because the row was since deleted, say): there is no row to take back.
    if (op.cancelled && isRejection(e) && !isUnauthenticated(e)) return null;
    throw e;
  }
  const row = answer.body;
  if (!row) return null;
  if (op.cancelled) {
    await deleteRow(rowPath(row.id));
    return null;
  }
  if (op.kind === 'activity.create' && op.tempId !== undefined) landedDrafts.set(op.tempId, row);
  if (op.foldedAt !== undefined && answer.status === 200) {
    await api('PATCH', rowPath(row.id), editBody(row), SAVE_TIMEOUT_MS);
  }
  return row;
}

/** DELETE that counts "already gone" as done. */
async function deleteRow(path: string): Promise<void> {
  try {
    await api('DELETE', path, undefined, SAVE_TIMEOUT_MS);
  } catch (e) {
    if (!(e instanceof ApiError && e.status === 404)) throw e;
  }
}

/**
 * The `edited_at` an edit to a draft's row is sent with: when the user made it, but never
 * before the row was made. The create stamps every field with the server's clock as it inserts (see
 * `record_create` in src/sync/record.rs), and a create that timed out may have been inserted
 * AFTER the edit -- a server slow enough to time out is one that can take that long. The
 * margin is for `created_at`, which has whole seconds only; the server caps it at its own now.
 */
function foldStamp(foldedAt: string, createdAt: string | undefined): string {
  const made = createdAt === undefined ? NaN : Date.parse(createdAt) + 2_000;
  return Number.isNaN(made) || Date.parse(foldedAt) > made ? foldedAt : new Date(made).toISOString();
}

async function doFlushOutbox(): Promise<void> {
  let resolved = new Map<number, number>();
  let before: string | null = null;
  let completed = false;
  let changed = true; // until a completed pass proves otherwise -- see the `finally` below
  try {
    // Nobody is signed in, so there is no session to attribute a send to and no way to tell
    // whose ops these are. `main.ts` flushes at module load -- before `App.svelte`'s onMount has
    // even called `loadSession`, which itself needs two round trips before it knows the user --
    // so without this the boot flush ran unattributed and, on a shared device, replayed one
    // user's queued writes under whoever's cookie happened to still be valid. The server
    // refuses them on ownership with a 404, which `replay` reads as permanent and parks them
    // dead: the exact loss the per-op owner exists to prevent, on the one flush that always
    // runs.
    //
    // Nothing is lost by waiting: `loadSession` and `login` both flush once the id is known,
    // and `loadSession` is retried on reconnect if its own first attempt failed. Inside the
    // `try` so the `finally` still notifies -- a listener that never hears from a skipped pass
    // is a view left showing whatever it last computed.
    //
    // The same holds for a user nobody has confirmed yet (offline mode): the flush that counts
    // is the one `loadSession` fires once the real session check succeeds.
    if (outboxUser() === null || !sendGateOpen()) { completed = true; changed = false; return; }
    before = await queueSnapshot();
    resolved = await replay(store, async (op: QueuedOp) => {
      if (!isOurs(op)) {
        // Someone else's queued write, waiting for them to sign back in on this device. Sending
        // it under the current session would get it refused on ownership and parked dead --
        // destroying their entry at the moment an unrelated person logged in. `SkipOp` leaves it
        // exactly as it is and lets the pass continue with our own ops.
        throw new SkipOp('outbox: op belongs to another user');
      }
      if (op.kind === 'attachment.upload') {
        if (!op.blob) {
          // Nothing to send and never will be -- this op can never succeed no matter how many
          // times it's retried. Treat it exactly like a permanent server rejection so it's
          // parked dead and the pass moves on, rather than being retried forever or, worse,
          // stopping every healthy op behind it.
          throw new ApiError(422, 'outbox_missing_file', 'queued upload has no file');
        }
        let activityId = op.body.activity_id;
        // Queued under a temp id that a pass in this tab has since landed, after that pass had
        // rewritten the create's children (see `landedDrafts`): the row is there to attach to.
        if (typeof activityId === 'number' && activityId < 0) activityId = landedDrafts.get(activityId)?.id ?? activityId;
        if (typeof activityId === 'number' && activityId < 0) {
          // A negative id is a placeholder for an `activity.create` that has never reached the
          // server -- sending it verbatim is a guaranteed 404. `replay` processes ops in queue
          // order and rewrites this field the instant the matching create resolves (see
          // `persistResolvedId`), so by the time this send is attempted the create ahead of it
          // has always already succeeded (and been rewritten) or gone permanently dead; a live
          // create still naming this same temp id existing at this exact moment is not
          // expected, but is what would make this "still waiting", not "orphaned".
          const stillPending = (await store.all())
            .some((o) => !o.dead && o.kind === 'activity.create' && o.tempId === activityId);
          if (stillPending) {
            // MINOR 2: this is not a failure of THIS op -- it did nothing wrong and has nothing
            // to retry yet -- so it must not burn an attempt or stop the pass the way an
            // ordinary thrown Error would (see `SkipOp` in `./outbox.ts`). The live create
            // ahead of it will get its own turn later in this very pass, or the next one.
            throw new SkipOp('outbox: upload is waiting on its parent activity.create');
          }
          // No live create will ever resolve this id: the parent was never created (or already
          // failed permanently) before this upload was queued. Park it dead with a legible
          // reason instead of burning an attempt on a send that can only ever 404.
          throw new ApiError(422, 'outbox_orphaned_activity', 'queued upload references an activity that was never created');
        }
        const form = new FormData();
        form.append('file', op.blob, op.filename ?? 'upload');
        form.append('client_op_id', op.id);
        if (typeof activityId === 'number') form.append('activity_id', String(activityId));
        const out = await upload<{ id: number }>(op.path, form);
        return out ?? null;
      }
      // The JSON sends below get the same deadline as a first attempt (see `SAVE_TIMEOUT_MS`):
      // a request that never answers would otherwise hold this pass, and every flush that joins
      // it, for as long as the connection takes to die. A timeout is an ordinary retryable
      // failure, and each of these is safe to repeat for the reasons given where it was queued.
      if (op.kind === 'activity.create') {
        return sendCreate(op, { ...op.body, client_op_id: op.id }, (id) => `/activities/${id}`,
          // An activity PATCH is a full body, and `edited_at` keeps it from beating a newer edit.
          (row) => ({ ...op.body, edited_at: foldStamp(op.foldedAt!, row.created_at) }));
      }
      if (op.kind === 'object.create') {
        return sendCreate(op, { ...op.body, client_uuid: op.id }, (id) => `/objects/${id}`, () => op.body);
      }
      if (op.kind === 'reminder.create') {
        const out = await api<{ id: number }>('POST', op.path, { ...op.body, client_uuid: op.id }, SAVE_TIMEOUT_MS);
        return out ?? null;
      }
      if (op.kind === 'activity.update') {
        // Replaying it twice is harmless: the second arrives with the same `edited_at`, which
        // does not beat the clock the first one left behind, so nothing changes.
        await api('PATCH', op.path, op.body, SAVE_TIMEOUT_MS);
        return null;
      }
      // A kind this function has no send path for at all (e.g. a queued 'reminder.done',
      // reserved for later) can never succeed no matter how many times it's retried -- treat
      // it exactly like a permanent server rejection: park it dead and let the pass continue,
      // rather than head-of-line-blocking every op behind it (see `replay` in ./outbox.ts).
      throw new ApiError(422, 'outbox_unsupported_kind', `flushOutbox: op kind "${op.kind}" has no send path`);
    }, { store: outboxLock.run, op: opLocks.run });
    completed = true;
  } finally {
    // Computed here, not after `replay` returns: a pass can throw AFTER it has already sent and
    // removed ops (an IndexedDB failure in the write-back), and reporting "nothing changed"
    // then leaves a view showing a synthetic pending row for a write that really did land --
    // neither pending any more nor ever refetched. A pass whose outcome cannot be established
    // says `changed`, which costs one refetch and never loses a row.
    try {
      // A pass that did not complete cannot say what it did, so it says "changed": one wasted
      // refetch, versus a view left showing a pending row for a write that already landed. A
      // pass skipped for want of a user completed and touched nothing, so `before` stays null
      // and it correctly reports no change.
      changed = !completed || (before !== null && (await queueSnapshot()) !== before);
    } catch {
      changed = true;
    }
    for (const fn of flushListeners) fn(resolved, changed);
  }
}

/** Send every queued write. Called at startup, on `online`, on `visibilitychange`, and after a
 *  manual retry -- several of which can land in the same tick, so this is `serialize`d (see
 *  `./outbox.ts`) rather than left to run overlapping passes over the same snapshot. */
export const flushOutbox = serialize(doFlushOutbox);

globalThis.addEventListener?.('online', () => { void flushOutbox(); });
// The common real outage -- a captive portal, weak signal, a 502 from a reverse proxy, the
// server restarting -- queues a write and then never fires `online` at all, so without this a
// queued op would sit until the user force-reloads the PWA. Reconnecting or backgrounding and
// returning to the tab is the moment a user actually finds out whether they're back online, so
// it doubles as a good trigger to retry.
globalThis.addEventListener?.('visibilitychange', () => {
  if (document.visibilityState === 'visible') void flushOutbox();
});

export async function outboxPending(): Promise<number> {
  return pendingCount(ourStore());
}

export async function deadOps(): Promise<QueuedOp[]> {
  return (await ourStore().all()).filter((o) => o.dead);
}

/** The store as the signed-in user sees it: their own ops only. Reads only -- a write still
 *  goes to the real store, since an op is addressed by its id and never by this view. */
function ourStore(): OutboxStore {
  return { ...store, all: async () => (await store.all()).filter(isOurs) };
}

/** How many ops are parked dead — a write the server has permanently refused and will never
 *  see again. Deliberately separate from `outboxPending`/`pendingCount`, which counts only ops
 *  still waiting to be sent: a dead op must not silently drop out of view just because it no
 *  longer counts as "pending", so callers show both rather than folding one into the other. */
export async function outboxDeadCount(): Promise<number> {
  return (await deadOps()).length;
}

/**
 * Ops still waiting to be sent (never the dead ones) whose target is `path` — used to show a
 * write made offline in the place it would otherwise appear once the server has it, so it is
 * never invisible in the meantime. No component reaches into the store directly.
 */
/** One owner-scoped queue read for a measurement history, independent of timeline filters. */
export async function pendingActivityOps(objectId: number, activityIds: number[]): Promise<QueuedOp[]> {
  const paths = new Set(activityIds.map(id => `/activities/${id}`));
  return (await store.all()).filter(o => !o.dead && !o.cancelled && isOurs(o) &&
    ((o.kind === 'activity.create' && o.path === `/objects/${objectId}/activities`) ||
     (o.kind === 'activity.update' && paths.has(o.path))));
}

export async function pendingOpsFor(path: string): Promise<QueuedOp[]> {
  return (await store.all()).filter((o) => !o.dead && !o.cancelled && o.path === path && isOurs(o));
}

/** Object creates waiting for this user's next successful connection, oldest first. */
export async function pendingObjectOps(): Promise<QueuedOp[]> {
  return (await ourStore().all()).filter((o) => !o.dead && !o.cancelled && o.kind === 'object.create');
}

export async function updateQueuedObject(tempId: number, body: Record<string, unknown>): Promise<boolean> {
  return underOpLocks(isQueuedObject(tempId), () => updateQueuedObjectBody(store, tempId, body, editedAtNow()));
}

export async function cancelQueuedObject(tempId: number): Promise<boolean> {
  return underOpLocks(isQueuedUnderObject(tempId), () => cancelQueuedObjectOps(store, tempId));
}

/**
 * Cancels a draft whose `activity.create` never reached the server -- only the outbox has it.
 * Removes that queued create and any upload still hanging off its temp id (see
 * `cancelQueuedActivityOps` in `./outbox.ts`), so the cancel leaves nothing behind to replay
 * later -- except a create that may have landed after all (it timed out, say): that one stays,
 * marked cancelled, until its replay has found the row and deleted it (see `sendCreate`). No
 * component reaches into the store directly.
 *
 * `tempId` may also be a real activity id -- see `removeQueuedActivity` -- so this doubles as
 * "drop any queued upload for this activity" when cancelling an already-synced row. Returns
 * whether anything was actually removed.
 *
 * Runs under the locks of the ops it removes (IMPORTANT 1), which `doFlushOutbox` holds while it
 * sends each one: without them, Cancel racing an in-flight `send()` could report the op removed
 * and then have the pass's own retry write-back, moments later, RESURRECT it -- the exact stray
 * entry this function exists to prevent. Waiting for that send to finish before this ever reads
 * the store means it always removes whatever the pass actually left behind, never something the
 * pass is about to overwrite.
 */
export async function cancelQueuedActivity(tempId: number): Promise<boolean> {
  if (await underOpLocks(isQueuedUnderActivity(tempId), () => cancelQueuedActivityOps(store, tempId))) return true;
  // A pass in this tab has landed the draft already (see `landedDrafts`): the row is real, and
  // is deleted like one -- the DELETE first, so a failure leaves its queued photos in place
  // with the row, and is thrown for the form to show rather than leaving it behind in silence.
  const real = landedDrafts.get(tempId)?.id;
  if (real === undefined) return false;
  await deleteRow(`/activities/${real}`);
  landedDrafts.delete(tempId);
  await underOpLocks(isQueuedUnderActivity(real), () => removeQueuedActivity(store, real));
  return true;
}

/**
 * Folds a further edit into a draft's still-queued `activity.create` (identified by its temp
 * id) instead of sending a PATCH the server has no row for yet. See `updateQueuedActivityBody`
 * in `./outbox.ts`. If a pass in this tab has already landed that create, the edit goes to the
 * real row through `updateQueued` instead. Returns whether the edit went anywhere at all:
 * false only when there is neither a live op nor a known row to give it to.
 *
 * Runs under the lock of the op it rewrites (IMPORTANT 1), which `doFlushOutbox` holds while it
 * sends that op: without it, Save racing an in-flight `send()` could report the edit folded in
 * and navigate away, and then have the pass's own retry write-back, moments later, overwrite
 * that edit with the stale body the in-flight request was actually sent with -- reverting a
 * save the user was just told succeeded. Waiting for that send to finish before this ever
 * writes means it always lands on top of whatever the pass actually did, never underneath it.
 * The send of any OTHER op -- a photo upload, typically -- does not hold it up.
 */
export async function updateQueuedActivity(tempId: number, body: Record<string, unknown>): Promise<boolean> {
  if (await underOpLocks(isQueuedActivity(tempId), () => updateQueuedActivityBody(store, tempId, body, editedAtNow()))) return true;
  const real = landedDrafts.get(tempId);
  if (real === undefined) return false;
  // Stamped no earlier than the row: it landed moments ago, perhaps (see `foldStamp`).
  await updateQueued(`/activities/${real.id}`, body, real.created_at);
  return true;
}

/**
 * Revive every parked op and try again — the user's "I fixed the wifi" button.
 *
 * MINOR 4(a): the loop above writes `dead: false` directly to the store, bypassing the lock --
 * so if a replay pass is already in flight when it runs, that pass's snapshot (taken from
 * `store.all()` before the revival) still marks the just-revived ops dead, and `flushOutbox`
 * below simply joins that already-running pass (`serialize` shares one in-flight run) instead
 * of starting a fresh one that would see them live. The user pressed "try again" and, silently,
 * nothing happened until some later, unrelated trigger fired. Awaiting `flushOutbox` once and
 * then calling it again guarantees the second call is a genuinely new pass (`serialize` only
 * dedupes calls that overlap an ALREADY-in-flight run; once the first call's promise settles,
 * `inFlight` is cleared) that reads the store as it stands right now, revived ops included --
 * whether the first call joined a stale pass or, the common case, was already a fresh one that
 * needed no help.
 */
export async function retryDead(): Promise<void> {
  // Only our own: another user's parked ops are not this user's to revive. They cannot see them
  // (`deadOps` filters) or send them (`doFlushOutbox` skips), so reviving them would just leave
  // ops live indefinitely and re-park them -- including ones their owner had given up on -- the
  // next time that owner signs in.
  //
  // `ourStore()` alone is not enough: `isOurs` widens to EVERY op when no user is known, which
  // is reachable if the 401 handler ends the session between Settings mounting and this click.
  // With nobody to revive them for -- and the flushes below no-ops in that state -- there is
  // nothing this could usefully do anyway.
  if (outboxUser() === null) return;
  for (const op of await ourStore().all()) {
    if (op.dead) await store.put({ ...op, dead: false, attempts: 0, lastError: undefined });
  }
  await flushOutbox();
  await flushOutbox();
}

/**
 * Permanently discards one dead op -- the user's "give up on this" button. Unlike `retryDead`,
 * this never attempts to resend it: a permanently-rejected `attachment.upload` holds its file
 * bytes (`QueuedOp.blob`) in IndexedDB, and until now Settings offered only "retry", so a dead
 * photo had no way to leave local storage short of the user clearing site data entirely.
 */
export async function discardDeadOp(id: string): Promise<void> {
  await store.remove(id);
}
