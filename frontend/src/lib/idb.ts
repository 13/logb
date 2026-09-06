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

function run<T>(mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return open().then((db) => new Promise<T>((resolve, reject) => {
    const req = fn(db.transaction(STORE, mode).objectStore(STORE));
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
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
