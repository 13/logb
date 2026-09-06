import { compareQueueOrder, type OutboxStore, type QueuedOp } from './outbox';

const DB = 'memto-outbox';
const STORE = 'ops';

/** One connection, opened lazily and reused for the life of the tab — every call used to open
 *  a fresh `IDBDatabase` and never close it, leaking one connection per TopBar mount and per
 *  'online' event. If opening ever fails, the cached promise is cleared so the next call gets
 *  a fresh attempt instead of a permanently rejected connection. */
let dbPromise: Promise<IDBDatabase> | null = null;

function open(): Promise<IDBDatabase> {
  if (!dbPromise) {
    dbPromise = new Promise((resolve, reject) => {
      const req = indexedDB.open(DB, 1);
      req.onupgradeneeded = () => req.result.createObjectStore(STORE, { keyPath: 'id' });
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => { dbPromise = null; reject(req.error); };
    });
  }
  return dbPromise;
}

/**
 * Resolves on the transaction's `oncomplete`, not the request's `onsuccess`: a request can
 * report success and then have its transaction abort anyway at commit time (e.g. a quota
 * failure) -- `onsuccess` alone would report the write as done while IndexedDB then discards
 * it, which the outbox would never learn about. This matters more now that queued ops can
 * carry megabyte blobs, making a quota abort at commit far more likely than it used to be.
 * `onerror` (the request failing outright) also aborts the transaction, so `onabort` alone is
 * enough to catch both.
 */
function run<T>(mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return open().then((db) => new Promise<T>((resolve, reject) => {
    const tx = db.transaction(STORE, mode);
    const req = fn(tx.objectStore(STORE));
    let result: T;
    req.onsuccess = () => { result = req.result; };
    tx.oncomplete = () => resolve(result);
    tx.onabort = () => reject(tx.error ?? req.error);
  }));
}

/**
 * The next `seq` to hand out (MINOR 3), lazily seeded from what's already in the store so a
 * freshly opened store never hands out a value smaller than one already written -- whether by
 * an earlier tab, an earlier page load, or (for a record written before `seq` existed) that
 * record's `queued_at`. Seeding past `queued_at` too, not just past existing `seq` values, is
 * what lets a legacy record compare correctly against a freshly-`seq`'d one in
 * `compareQueueOrder` without ever needing to be rewritten itself: `queued_at` is already an
 * epoch-millisecond timestamp, so seeding here starts the counter at roughly "now" and counts
 * up from there, strictly past every legacy value.
 *
 * A module-level promise, not a plain number, so that concurrent `put`s -- e.g. attaching
 * several files in one go -- each reserve a distinct value by chaining onto it, rather than two
 * calls racing to read the same "current max" and handing out the same `seq` twice.
 */
let nextSeq: Promise<number> | null = null;

function reserveSeq(): Promise<number> {
  if (!nextSeq) {
    nextSeq = run<QueuedOp[]>('readonly', (s) => s.getAll() as IDBRequest<QueuedOp[]>)
      .then((rows) => 1 + rows.reduce((max, r) => Math.max(max, r.seq ?? r.queued_at ?? 0), 0));
  }
  const reserved = nextSeq;
  nextSeq = reserved.then((n) => n + 1);
  return reserved;
}

/**
 * Insertion order is preserved: keys are UUIDs, so `getAll()` returns them in key order, which
 * carries no relation to enqueue order -- and two ops enqueued in the same millisecond would
 * otherwise tie on `queued_at` too, leaving their relative order to that same meaningless key
 * order (MINOR 3). `seq`, assigned once per op in `put` below and preserved on every later
 * rewrite of the same op (`op.seq ?? await reserveSeq()`; see `QueuedOp.seq`), is what actually
 * breaks that tie deterministically; `compareQueueOrder` falls back to `queued_at` only for a
 * record written before this field existed.
 */
export function idbStore(): OutboxStore {
  return {
    async all() {
      const rows = await run<QueuedOp[]>('readonly', (s) => s.getAll() as IDBRequest<QueuedOp[]>);
      return rows.sort(compareQueueOrder);
    },
    async put(op) {
      const seq = op.seq ?? await reserveSeq();
      await run('readwrite', (s) => s.put({ queued_at: Date.now(), ...op, seq }));
    },
    async remove(id) { await run('readwrite', (s) => s.delete(id)); },
  };
}
