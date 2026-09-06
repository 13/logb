import { enqueue, newOpId, pendingCount, replay, serialize, type OutboxStore, type QueuedOp } from './outbox';
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

/** Notified after each `flushOutbox()` pass completes, so a mounted view can drop a synthetic
 *  pending entry the instant its real row lands instead of showing it until the next remount. */
const flushListeners = new Set<() => void>();
export function onOutboxFlushed(fn: () => void): () => void {
  flushListeners.add(fn);
  return () => flushListeners.delete(fn);
}

async function doFlushOutbox(): Promise<void> {
  try {
    await replay(store, async (op: QueuedOp) => {
      if (op.kind !== 'activity.create') {
        // Only 'activity.create' is ever queued today (see createQueued above) and it is the
        // only kind this function knows how to resend as JSON. An 'attachment.upload' op
        // carries a Blob, which JSON.stringify silently turns into `{}` -- refuse loudly
        // instead of corrupting the upload, so wiring up queued uploads later requires giving
        // this a real multipart path rather than tripping over a silent data-loss bug.
        throw new Error(`flushOutbox: op kind "${op.kind}" has no send path yet`);
      }
      const out = await api<{ id: number }>('POST', op.path, { ...op.body, client_op_id: op.id });
      return out ?? null;
    });
  } finally {
    for (const fn of flushListeners) fn();
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

/** Revive every parked op and try again — the user's "I fixed the wifi" button. */
export async function retryDead(): Promise<void> {
  for (const op of await store.all()) {
    if (op.dead) await store.put({ ...op, dead: false, attempts: 0 });
  }
  await flushOutbox();
}
