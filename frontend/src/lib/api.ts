import { enqueue, newOpId, pendingCount, removeQueuedActivity, replay, serialize, updateQueuedActivityBody, type OutboxStore, type QueuedOp } from './outbox';
import { idbStore } from './idb';
import { ApiError, isRejection } from './api-error';

export { ApiError, isRejection } from './api-error';

let onUnauthorized: () => void = () => {};
export function setUnauthorizedHandler(fn: () => void): void {
  onUnauthorized = fn;
}

async function handle<T>(res: Response, path: string): Promise<T> {
  if (res.status === 204) return undefined as T;
  const isJson = (res.headers.get('content-type') ?? '').includes('application/json');
  const body = isJson ? await res.json() : null;
  if (!res.ok) {
    if (res.status === 401 && !path.startsWith('/auth')) onUnauthorized();
    throw new ApiError(res.status, body?.error ?? 'error', body?.message ?? `HTTP ${res.status}`);
  }
  return body as T;
}

export async function api<T = unknown>(method: string, path: string, body?: unknown): Promise<T> {
  const init: RequestInit = { method, credentials: 'same-origin', headers: {} };
  if (body !== undefined) {
    init.headers = { 'content-type': 'application/json' };
    init.body = JSON.stringify(body);
  }
  const res = await fetch(`/api${path}`, init);
  return handle<T>(res, path);
}

/**
 * A GET whose response is a page of a longer list. `total` comes from `X-Total-Count` and
 * counts everything matching the filters, not just this page, so the caller knows whether
 * there is more to fetch.
 */
export async function apiPage<T = unknown>(path: string): Promise<{ items: T[]; total: number }> {
  const res = await fetch(`/api${path}`, { method: 'GET', credentials: 'same-origin' });
  const items = await handle<T[]>(res, path);
  const header = res.headers.get('x-total-count');
  const total = header === null ? items.length : Number(header);
  return { items, total: Number.isFinite(total) ? total : items.length };
}

export async function upload<T = unknown>(path: string, form: FormData): Promise<T> {
  const res = await fetch(`/api${path}`, { method: 'POST', credentials: 'same-origin', body: form });
  return handle<T>(res, path);
}

export async function uploadRaw<T = unknown>(path: string, blob: Blob, contentType: string): Promise<T> {
  const res = await fetch(`/api${path}`, { method: 'POST', credentials: 'same-origin', headers: { 'content-type': contentType }, body: blob });
  return handle<T>(res, path);
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
  try {
    return await api<T>('POST', path, { ...body, client_op_id: id });
  } catch (e) {
    if (isRejection(e)) throw e;
    try {
      await enqueue(store, { id, kind: 'activity.create', path, body, tempId, attempts: 0 });
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
      if (isRejection(e)) throw e;
      // Fall through to queue, same as createQueued.
    }
  }
  try {
    await enqueue(store, { id, kind: 'attachment.upload', path, body, blob: file, filename, attempts: 0 });
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
 *  without re-deriving it from the store -- an empty map on a pass that resolved nothing. */
const flushListeners = new Set<(resolved: Map<number, number>) => void>();
export function onOutboxFlushed(fn: (resolved: Map<number, number>) => void): () => void {
  flushListeners.add(fn);
  return () => flushListeners.delete(fn);
}

async function doFlushOutbox(): Promise<void> {
  let resolved = new Map<number, number>();
  try {
    resolved = await replay(store, async (op: QueuedOp) => {
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
            throw new Error('outbox: upload is waiting on its parent activity.create');
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
      // A kind this function has no send path for at all (e.g. a queued 'reminder.done',
      // reserved for later) can never succeed no matter how many times it's retried -- treat
      // it exactly like a permanent server rejection: park it dead and let the pass continue,
      // rather than head-of-line-blocking every op behind it (see `replay` in ./outbox.ts).
      throw new ApiError(422, 'outbox_unsupported_kind', `flushOutbox: op kind "${op.kind}" has no send path`);
    });
  } finally {
    for (const fn of flushListeners) fn(resolved);
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

export function outboxPending(): Promise<number> {
  return pendingCount(store);
}

export async function deadOps(): Promise<QueuedOp[]> {
  return (await store.all()).filter((o) => o.dead);
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
  return (await store.all()).filter((o) => !o.dead && o.path === path);
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
 */
export async function cancelQueuedActivity(tempId: number): Promise<boolean> {
  return removeQueuedActivity(store, tempId);
}

/**
 * Folds a further edit into a draft's still-queued `activity.create` (identified by its temp
 * id) instead of sending a PATCH the server has no row for yet. See `updateQueuedActivityBody`
 * in `./outbox.ts`. Returns whether a live op was actually found and rewritten.
 */
export async function updateQueuedActivity(tempId: number, body: Record<string, unknown>): Promise<boolean> {
  return updateQueuedActivityBody(store, tempId, body);
}

/** Revive every parked op and try again — the user's "I fixed the wifi" button. */
export async function retryDead(): Promise<void> {
  for (const op of await store.all()) {
    if (op.dead) await store.put({ ...op, dead: false, attempts: 0 });
  }
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
