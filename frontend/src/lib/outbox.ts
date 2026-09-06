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

export async function enqueue(store: OutboxStore, op: QueuedOp): Promise<void> {
  await store.put(op);
}

export async function pendingCount(store: OutboxStore): Promise<number> {
  return (await store.all()).filter((o) => !o.dead).length;
}

/**
 * Send every live op in order, oldest first, stopping at the first failure so ops that
 * depend on an earlier one cannot overtake it.
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
      if (op.tempId !== undefined && out) resolved.set(op.tempId, out.id);
      await store.remove(op.id);
    } catch {
      const attempts = op.attempts + 1;
      await store.put({ ...op, attempts, dead: attempts >= MAX_ATTEMPTS });
      return;
    }
  }
}
