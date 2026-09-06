import type { OutboxStore, QueuedOp } from './outbox';

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

/** Insertion order is preserved: keys are UUIDs, so the queue keeps its own `queued_at`. */
export function idbStore(): OutboxStore {
  return {
    async all() {
      const rows = await run<QueuedOp[]>('readonly', (s) => s.getAll() as IDBRequest<QueuedOp[]>);
      return rows.sort((a, b) => (a.queued_at ?? 0) - (b.queued_at ?? 0));
    },
    async put(op) { await run('readwrite', (s) => s.put({ queued_at: Date.now(), ...op })); },
    async remove(id) { await run('readwrite', (s) => s.delete(id)); },
  };
}
