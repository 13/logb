import { isRejection } from './api-error';

/**
 * The queue of writes made while offline.
 *
 * Creates only. An edit or a delete queued offline would have to be reconciled against
 * whatever the server did in the meantime; a create cannot disagree with anything, which
 * is why the offline story stops here rather than growing a merge algorithm.
 */
export type OpKind = 'activity.create' | 'attachment.upload' | 'reminder.done';

export interface QueuedOp {
  /** Also the `client_op_id` sent to the server, which is what makes a replay idempotent. */
  id: string;
  kind: OpKind;
  path: string;
  body: Record<string, unknown>;
  blob?: Blob;
  /** Original filename for `blob`, as its own field rather than read off `blob.name`: a plain
   *  `Blob` has no `.name` at all (only a `File` does), so a send path that needs a filename
   *  for every queued upload -- not just ones that happened to be picked as a `File` -- needs
   *  it recorded explicitly. (A `File`'s own `.name` does survive IndexedDB's structured-clone
   *  storage, so this is about typing/uniformity, not working around data loss.) */
  filename?: string;
  /** Negative placeholder id this op's created row is known by until the server answers. */
  tempId?: number;
  attempts: number;
  dead?: boolean;
  /** IndexedDB insertion order marker. The memory store does not need this because arrays
   *  already keep insertion order; the IndexedDB store stamps it itself on `put`. */
  queued_at?: number;
}

export interface OutboxStore {
  all(): Promise<QueuedOp[]>;
  put(op: QueuedOp): Promise<void>;
  remove(id: string): Promise<void>;
}

const MAX_ATTEMPTS = 3;

/** An in-memory store, for tests. */
export function memoryStore(): OutboxStore {
  const rows: QueuedOp[] = [];
  return {
    async all() { return rows.map((r) => ({ ...r })); },
    async put(op) {
      const i = rows.findIndex((r) => r.id === op.id);
      if (i === -1) rows.push({ ...op }); else rows[i] = { ...op };
    },
    async remove(id) {
      const i = rows.findIndex((r) => r.id === id);
      if (i !== -1) rows.splice(i, 1);
    },
  };
}

export function newOpId(): string {
  return globalThis.crypto.randomUUID();
}

/**
 * Wraps an async function so overlapping calls share one in-flight run instead of each
 * starting a fresh one. `flushOutbox` (`./api.ts`) is called at startup, on `online`, on
 * `visibilitychange`, and after a manual retry -- several of which can fire within the same
 * tick. Without this, two overlapping passes over one outbox snapshot can race: if pass 1
 * finishes and removes an op while pass 2 is still mid-flight (having read that op before pass
 * 1 removed it), pass 2 goes on to write it back with `attempts: 1`, resurrecting a completed
 * op as a phantom pending row. Once the wrapped call settles, the next call starts a genuinely
 * new run rather than replaying a stale result.
 */
export function serialize<T>(fn: () => Promise<T>): () => Promise<T> {
  let inFlight: Promise<T> | null = null;
  return () => {
    if (!inFlight) inFlight = fn().finally(() => { inFlight = null; });
    return inFlight;
  };
}

export async function enqueue(store: OutboxStore, op: QueuedOp): Promise<void> {
  await store.put(op);
}

export async function pendingCount(store: OutboxStore): Promise<number> {
  return (await store.all()).filter((o) => !o.dead).length;
}

/**
 * Send every live op in order, oldest first.
 *
 * A 4xx `ApiError` means the server has permanently refused this op — retrying it would only
 * get the same answer again, and this queue is FIFO, so leaving it at the head would also
 * head-of-line-block every healthy op behind it. So that case is parked as dead immediately
 * and the pass continues with the rest of the queue.
 *
 * Any other failure (network drop, 5xx, ...) means we don't know whether the server saw this
 * op at all, so it stays queued and the pass stops right there: ops further back may depend on
 * this one (see the `activity_id` rewrite below) and must not be sent out of order ahead of it.
 */
export async function replay(
  store: OutboxStore,
  send: (op: QueuedOp) => Promise<{ id: number } | null>,
): Promise<void> {
  const resolved = new Map<number, number>();
  for (const op of await store.all()) {
    if (op.dead) continue;
    const body = { ...op.body };
    const ref = body.activity_id;
    if (typeof ref === 'number' && resolved.has(ref)) body.activity_id = resolved.get(ref);
    try {
      const out = await send({ ...op, body });
      if (op.tempId !== undefined && out) {
        resolved.set(op.tempId, out.id);
        await persistResolvedId(store, op.tempId, out.id);
      }
      await store.remove(op.id);
    } catch (e) {
      if (isRejection(e)) {
        await store.put({ ...op, dead: true });
        continue;
      }
      const attempts = op.attempts + 1;
      await store.put({ ...op, attempts, dead: attempts >= MAX_ATTEMPTS });
      return;
    }
  }
}

/**
 * Once a create's temp id resolves to a real one, write the real id into every OTHER live
 * stored op that still names the temp id -- not only into this call's in-memory `resolved`
 * map, which dies with the function.
 *
 * Why this has to reach the store and not just the map: a pass that resolves a create and
 * then stops (a later, unrelated op fails and `replay` returns before reaching the dependent
 * op) leaves that dependent op -- e.g. an `attachment.upload` queued behind the create -- still
 * holding the temp id, but only in memory that is about to be discarded. The next call to
 * `replay` starts a brand-new, empty `resolved` map and reads the dependent op straight back
 * out of the store; if the store still says `activity_id: <temp id>`, that negative placeholder
 * -- an id no row will ever actually have -- goes out on the wire verbatim. Writing the real id
 * into the store the moment it's known means a later pass never needs the map to see it: the
 * stored op already names the real activity, so there is nothing left to resolve and nothing
 * that can regress to sending the temp id.
 */
async function persistResolvedId(store: OutboxStore, tempId: number, realId: number): Promise<void> {
  for (const other of await store.all()) {
    if (other.dead || other.body.activity_id !== tempId) continue;
    await store.put({ ...other, body: { ...other.body, activity_id: realId } });
  }
}
