import { enqueue, newOpId, pendingCount, replay, type OutboxStore, type QueuedOp } from './outbox';
import { idbStore } from './idb';

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) {
    super(message);
  }
}

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

const store: OutboxStore = idbStore();

/** True for "the request never reached the server", false for "the server said no". */
function isOffline(e: unknown): boolean {
  return !navigator.onLine || e instanceof TypeError;
}

/**
 * POST that survives a dead connection: on a network failure the op is queued and replayed
 * later. `tempId` is the placeholder the caller shows in the meantime. The `client_op_id` is
 * generated once here, not per attempt, so every retry of this op — including the one sent
 * later by `replay` — carries the same id and a lost response can never duplicate the row.
 */
export async function createQueued<T>(path: string, body: Record<string, unknown>, tempId?: number): Promise<T | null> {
  const id = newOpId();
  try {
    return await api<T>('POST', path, { ...body, client_op_id: id });
  } catch (e) {
    if (!isOffline(e)) throw e;
    await enqueue(store, { id, kind: 'activity.create', path, body, tempId, attempts: 0 });
    return null;
  }
}

export function flushOutbox(): Promise<void> {
  return replay(store, async (op: QueuedOp) => {
    const out = await api<{ id: number }>('POST', op.path, { ...op.body, client_op_id: op.id });
    return out ?? null;
  });
}

globalThis.addEventListener?.('online', () => { void flushOutbox(); });

export function outboxPending(): Promise<number> {
  return pendingCount(store);
}

export async function deadOps(): Promise<QueuedOp[]> {
  return (await store.all()).filter((o) => o.dead);
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
