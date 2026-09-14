import { readonly, writable, type Readable } from 'svelte/store';
import { createLock, enqueue, newOpId, pendingCount, removeQueuedActivity, replay, serialize, SkipOp, updateQueuedActivityBody, type OutboxStore, type QueuedOp } from './outbox';
import { idbStore } from './idb';
import { ApiError, isRejection, isUnauthenticated } from './api-error';
import { path as routerPath } from './router';

export { ApiError, isRejection, isUnauthenticated } from './api-error';

/**
 * Which request paths (exactly as passed to `api()`/`apiPage()`/etc, query string included) are
 * currently answering from the service worker's cache rather than the network -- see
 * `servedFromCache` below. A per-path SET, not one flag: most screens have several requests in
 * flight at once (the dashboard alone fires five), and a single "last response wins" flag used to
 * flap back to false the instant any ONE of them came back fresh -- an uncached `/settings`
 * answering in milliseconds, say -- while another was still visibly showing data from `logb-api`.
 *
 * A key is added by a stale response and removed only by a FRESH response to that SAME path,
 * never by an unrelated one. The whole set is dropped on a route change (`clearServingSaved`,
 * wired to the router's `path` below -- each screen re-fetches what it needs, so staleness
 * recorded for the previous screen's requests stops being meaningful) and when a session ends
 * (same function, called from `endSession` in ../stores/session.ts).
 */
const staleKeys = new Set<string>();
const servingSavedState = writable(false);
export const servingSaved: Readable<boolean> = readonly(servingSavedState);

/** Drops all tracked staleness and hides the note. Idempotent, so calling it when nothing is
 *  stale (the common case) does not needlessly re-notify `servingSaved`'s subscribers. */
export function clearServingSaved(): void {
  if (staleKeys.size === 0) return;
  staleKeys.clear();
  servingSavedState.set(false);
}
// Each screen re-fetches what it shows on mount, so navigating away makes any staleness recorded
// for the PREVIOUS screen's requests meaningless -- without this, a note earned by one slow load
// on the objects list would keep showing on a completely unrelated screen that never touched that
// path. `path` only changes on a real `popstate` (see ./router.ts), which needs a DOM `window` --
// this subscription is inert, harmlessly, under Vitest's node test environment; `clearServingSaved`
// is exported so a test can simulate a route change directly instead.
routerPath.subscribe(() => clearServingSaved());

/**
 * How far the SERVER's clock reads from this device's, in ms (positive: the server is behind).
 * A self-hosted instance with no RTC (a Raspberry Pi that boots believing it's 1970, or simply the
 * wrong timezone) can be off by far more than the 60s threshold below, in either direction --
 * which would otherwise show the note permanently (server ahead of us) or hide a genuine cache hit
 * forever (server behind us, so a stale cached response still looks "recent enough").
 *
 * Calibrated from responses that can NEVER be a cache hit: `/auth/...` and `/settings` are
 * `NetworkOnly` (see vite.config.ts), so their `Date` header always reflects a live request made
 * moments ago -- any gap between it and `sentAt` is clock skew, not cache age. Both are requested
 * on every session check, so this recalibrates itself continuously rather than trusting one
 * reading for the life of the tab.
 */
let clockSkewMs = 0;

/** Test seam only: resets the calibrated skew between tests that exercise it through
 *  `api()`/`handle()`, so one test's calibration cannot leak into the next. Production never
 *  calls this -- the module starts at 0 and only ever recalibrates from a real response. */
export function resetClockSkewForTesting(): void {
  clockSkewMs = 0;
}

/**
 * Pure so it can be unit-tested without a fetch: true only when `dateHeader` is far enough before
 * `sentAt` (the moment the request went out), once `skewMs` -- the server clock's known offset
 * from this device's, see `clockSkewMs` above -- is subtracted out, that ordinary latency cannot
 * explain it. Given the 4s `NetworkFirst` timeout, only a cache hit reads as over a minute stale
 * after that correction. A missing or unparsable header counts as fresh: there is nothing there to
 * prove otherwise, and treating "we don't know" as "cached" would show the note on every response
 * an unrelated proxy happened to strip the header from.
 */
export function servedFromCache(dateHeader: string | null, sentAt: number, skewMs: number = 0): boolean {
  if (dateHeader === null) return false;
  const headerTime = Date.parse(dateHeader);
  if (Number.isNaN(headerTime)) return false;
  return sentAt - headerTime - skewMs > 60_000;
}

let onUnauthorized: () => void = () => {};
export function setUnauthorizedHandler(fn: () => void): void {
  onUnauthorized = fn;
}

/**
 * Who the queue currently belongs to. The outbox is one IndexedDB per origin, shared by every
 * account that signs in on the device, and a queued write deliberately outlives the session
 * that made it -- so on a shared device the next person to log in would otherwise replay
 * another user's writes under their own session (the server refuses them on ownership, and
 * `replay` then parks them permanently dead), and see their titles in the pending and failed
 * lists. Set from `../stores/session.ts` on every path that establishes or ends a session.
 */
let currentUserId: number | null = null;
export function setOutboxUser(id: number | null): void {
  currentUserId = id;
}

/**
 * Whether a server has actually confirmed the session the outbox user comes from. In offline
 * mode (see `../stores/session.ts`) the user is the last one remembered on this device, not one
 * the server has vouched for: their writes may queue, but sending them under whatever cookie is
 * there now could replay them as someone else. Injected rather than imported so `api.ts` keeps
 * not depending on the session module. Defaults to "confirmed" so code that sets an outbox user
 * directly (the outbox tests) behaves as before.
 */
let sessionConfirmed: () => boolean = () => true;
export function setOutboxSendGate(fn: () => boolean): void {
  sessionConfirmed = fn;
}

/** An op belongs to the session in front of us unless it is demonstrably someone else's. A
 *  record queued before `userId` existed, or one read while no user is known, counts as ours --
 *  the alternative is stranding a write nobody can ever see.
 *
 *  For DISPLAY only. Sending is stricter: see the guard at the top of `doFlushOutbox`. */
function isOurs(op: QueuedOp): boolean {
  return op.userId === undefined || currentUserId === null || op.userId === currentUserId;
}

/**
 * `sentAt` is optional only so this stays callable without it (there is no response to have come
 * from a cache before one exists); every real call site below passes it. Recorded here, the one
 * place every fetch wrapper's response passes through, rather than in each of them, so every path
 * is tracked the same way regardless of which wrapper fetched it.
 */
async function handle<T>(res: Response, path: string, sentAt?: number): Promise<T> {
  if (sentAt !== undefined && res.ok) {
    const dateHeader = res.headers.get('date');
    // Calibrate the clock-skew estimate from a response that is never a cache hit (see
    // `clockSkewMs`) BEFORE judging this one -- harmless for an auth/settings response itself
    // (never cacheable, so never added to `staleKeys` regardless), but keeps every path's
    // judgement working off the freshest skew reading available.
    if ((path.startsWith('/auth/') || path.startsWith('/settings')) && dateHeader !== null) {
      const t = Date.parse(dateHeader);
      if (!Number.isNaN(t)) clockSkewMs = sentAt - t;
    }
    if (servedFromCache(dateHeader, sentAt, clockSkewMs)) staleKeys.add(path);
    else staleKeys.delete(path);
    servingSavedState.set(staleKeys.size > 0);
  }
  if (res.status === 204) return undefined as T;
  const isJson = (res.headers.get('content-type') ?? '').includes('application/json');
  const body = isJson ? await res.json() : null;
  if (!res.ok) {
    if (res.status === 401 && !path.startsWith('/auth')) onUnauthorized();
    throw new ApiError(res.status, body?.error ?? 'error', body?.message ?? `HTTP ${res.status}`, body);
  }
  return body as T;
}

/**
 * `timeoutMs` is for the rare request that is *meant* to hold its connection open for a long
 * time -- today only the database switch, which answers once the whole copy has been made and
 * verified. `fetch` imposes no deadline of its own, so the default stays "as long as the
 * network allows"; a caller that passes one is choosing a ceiling far above the work, not a
 * normal request timeout. Setting it too low is the expensive mistake: the copy would keep
 * running server-side after the client gave up, and the operator would be told a migration
 * failed that in fact succeeded.
 */
export async function api<T = unknown>(method: string, path: string, body?: unknown, timeoutMs?: number): Promise<T> {
  const init: RequestInit = { method, credentials: 'same-origin', headers: {} };
  if (timeoutMs !== undefined) init.signal = AbortSignal.timeout(timeoutMs);
  if (body !== undefined) {
    init.headers = { 'content-type': 'application/json' };
    init.body = JSON.stringify(body);
  }
  const sentAt = Date.now();
  const res = await fetch(`/api${path}`, init);
  return handle<T>(res, path, sentAt);
}

/**
 * A GET whose response is a page of a longer list. `total` comes from `X-Total-Count` and
 * counts everything matching the filters, not just this page, so the caller knows whether
 * there is more to fetch.
 */
export async function apiPage<T = unknown>(path: string): Promise<{ items: T[]; total: number }> {
  const sentAt = Date.now();
  const res = await fetch(`/api${path}`, { method: 'GET', credentials: 'same-origin' });
  const items = await handle<T[]>(res, path, sentAt);
  const header = res.headers.get('x-total-count');
  const total = header === null ? items.length : Number(header);
  return { items, total: Number.isFinite(total) ? total : items.length };
}

export async function upload<T = unknown>(path: string, form: FormData): Promise<T> {
  const sentAt = Date.now();
  const res = await fetch(`/api${path}`, { method: 'POST', credentials: 'same-origin', body: form });
  return handle<T>(res, path, sentAt);
}

export async function uploadRaw<T = unknown>(path: string, blob: Blob, contentType: string): Promise<T> {
  const sentAt = Date.now();
  const res = await fetch(`/api${path}`, { method: 'POST', credentials: 'same-origin', headers: { 'content-type': contentType }, body: blob });
  return handle<T>(res, path, sentAt);
}

export function fileUrl(fileId: number, thumb = false): string {
  return `/api/files/${fileId}${thumb ? '/thumb' : ''}`;
}

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
  const userId = currentUserId ?? undefined;
  try {
    return await api<T>('POST', path, { ...body, client_op_id: id });
  } catch (e) {
    // A 401 is a rejection the user can undo by logging back in, so the write is queued rather
    // than thrown away -- the same reasoning `replay` applies to an op that meets an expired
    // session mid-pass (see `isUnauthenticated` in ./api-error.ts). Without this, a session
    // that lapsed while the form was open discarded everything typed into it: the unauthorized
    // handler navigates to /login, and the entry existed nowhere else.
    if (isRejection(e) && !isUnauthenticated(e)) throw e;
    try {
      await enqueue(store, { id, kind: 'activity.create', path, body, tempId, attempts: 0, userId });
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

/**
 * PATCH that survives a dead connection, for an edit to a row the server already has. Resolves
 * `true` when the server took it, `false` when it only reached the queue.
 *
 * The queued body carries `edited_at` -- the moment the person made the edit, not the moment
 * it is finally sent -- so the server can keep a newer change someone else made in the meantime
 * instead of overwriting it with an older one (see `edited_at` on `ActivityInput`,
 * src/api/activities.rs). An edit that goes straight through needs no such stamp: now is when
 * it was made.
 */
export async function updateQueued(path: string, body: Record<string, unknown>): Promise<boolean> {
  const id = newOpId();
  const editedAt = new Date().toISOString();
  // Before the request, for the same reason as in `createQueued`.
  const userId = currentUserId ?? undefined;
  try {
    await api('PATCH', path, body);
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
  const userId = currentUserId ?? undefined; // see createQueued: read before the request, not after

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
    await enqueue(store, { id, kind: 'attachment.upload', path, body, blob: file, filename, attempts: 0, userId });
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
 * the other is allowed to touch the store at all. IMPORTANT 1: without this, a Save or Cancel
 * fired while a pass's `send()` is in flight raced the pass's own eventual write-back and lost
 * -- the pass, snapshotting the op before the UI write happened, would win by writing back
 * exactly what the user had just changed or removed, moments after telling them it worked.
 *
 * Named, so the lock spans TABS as well as callers: the store it guards is IndexedDB, which
 * every same-origin tab shares, so a second tab replaying the same queue reproduces the same
 * interleaving between tabs that this lock closes within one. On a browser with no Web Locks
 * API the name is inert and the exclusion stays tab-local, as it was before (see `createLock`).
 */
const outboxLock = createLock('logb-outbox');

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
    if (currentUserId === null || !sessionConfirmed()) { completed = true; changed = false; return; }
    before = await queueSnapshot();
    resolved = await outboxLock.run(() => replay(store, async (op: QueuedOp) => {
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
        const activityId = op.body.activity_id;
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
      if (op.kind === 'activity.create') {
        const out = await api<{ id: number }>('POST', op.path, { ...op.body, client_op_id: op.id });
        return out ?? null;
      }
      if (op.kind === 'activity.update') {
        // Replaying it twice is harmless: the second arrives with the same `edited_at`, which
        // does not beat the clock the first one left behind, so nothing changes.
        await api('PATCH', op.path, op.body);
        return null;
      }
      // A kind this function has no send path for at all (e.g. a queued 'reminder.done',
      // reserved for later) can never succeed no matter how many times it's retried -- treat
      // it exactly like a permanent server rejection: park it dead and let the pass continue,
      // rather than head-of-line-blocking every op behind it (see `replay` in ./outbox.ts).
      throw new ApiError(422, 'outbox_unsupported_kind', `flushOutbox: op kind "${op.kind}" has no send path`);
    }));
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
export async function pendingOpsFor(path: string): Promise<QueuedOp[]> {
  return (await store.all()).filter((o) => !o.dead && o.path === path && isOurs(o));
}

/**
 * Cancels a draft whose `activity.create` never reached the server -- only the outbox has it.
 * Removes that queued create and any upload still hanging off its temp id (see
 * `removeQueuedActivity` in `./outbox.ts`), so the cancel leaves nothing behind to replay
 * later. No component reaches into the store directly.
 *
 * `tempId` may also be a real activity id -- see `removeQueuedActivity` -- so this doubles as
 * "drop any queued upload for this activity" when cancelling an already-synced row. Returns
 * whether anything was actually removed.
 *
 * Runs under `outboxLock` (IMPORTANT 1), the same lock `doFlushOutbox` holds for the whole of
 * its replay pass: without it, Cancel racing an in-flight `send()` could report the op removed
 * and then have the pass's own retry write-back, moments later, RESURRECT it -- the exact stray
 * entry this function exists to prevent. Waiting for the pass to finish before this ever reads
 * the store means it always removes whatever the pass actually left behind, never something the
 * pass is about to overwrite.
 */
export async function cancelQueuedActivity(tempId: number): Promise<boolean> {
  return outboxLock.run(() => removeQueuedActivity(store, tempId));
}

/**
 * Folds a further edit into a draft's still-queued `activity.create` (identified by its temp
 * id) instead of sending a PATCH the server has no row for yet. See `updateQueuedActivityBody`
 * in `./outbox.ts`. Returns whether a live op was actually found and rewritten.
 *
 * Runs under `outboxLock` (IMPORTANT 1), the same lock `doFlushOutbox` holds for the whole of
 * its replay pass: without it, Save racing an in-flight `send()` could report the edit folded
 * in and navigate away, and then have the pass's own retry write-back, moments later, overwrite
 * that edit with the stale body the in-flight request was actually sent with -- reverting a
 * save the user was just told succeeded. Waiting for the pass to finish before this ever writes
 * means it always lands on top of whatever the pass actually did, never underneath it.
 */
export async function updateQueuedActivity(tempId: number, body: Record<string, unknown>): Promise<boolean> {
  return outboxLock.run(() => updateQueuedActivityBody(store, tempId, body));
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
  if (currentUserId === null) return;
  for (const op of await ourStore().all()) {
    if (op.dead) await store.put({ ...op, dead: false, attempts: 0 });
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
