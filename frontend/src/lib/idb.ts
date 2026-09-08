import { compareQueueOrder, type OutboxStore, type QueuedOp } from './outbox';

const DB = 'logby-outbox';
const STORE = 'ops';
const SEQ = 'seq';

/** One connection, opened lazily and reused for the life of the tab — every call used to open
 *  a fresh `IDBDatabase` and never close it, leaking one connection per TopBar mount and per
 *  'online' event. If opening ever fails, the cached promise is cleared so the next call gets
 *  a fresh attempt instead of a permanently rejected connection. */
let dbPromise: Promise<IDBDatabase> | null = null;

function open(): Promise<IDBDatabase> {
  if (!dbPromise) {
    dbPromise = new Promise((resolve, reject) => {
      const req = indexedDB.open(DB, 2);
      req.onupgradeneeded = (e) => {
        const db = req.result;
        const tx = req.transaction!;
        const s = db.objectStoreNames.contains(STORE)
          ? tx.objectStore(STORE)
          : db.createObjectStore(STORE, { keyPath: 'id' });
        if (!s.indexNames.contains(SEQ)) s.createIndex(SEQ, SEQ);
        // v1 -> v2: give every record written before the index existed a `seq`, so the index
        // covers the whole store (IndexedDB leaves a record out of an index entirely when the
        // indexed field is missing). `queued_at` is epoch milliseconds and was already what
        // `compareQueueOrder` fell back to for these, so copying it preserves their order
        // against each other and against everything queued since.
        if (e.oldVersion >= 1) {
          const cur = s.openCursor();
          cur.onsuccess = () => {
            const c = cur.result;
            if (!c) return;
            const row = c.value as QueuedOp;
            if (row.seq === undefined) c.update({ ...row, seq: row.queued_at ?? 0 });
            c.continue();
          };
        }
      };
      req.onsuccess = () => {
        const db = req.result;
        // Another tab wants to upgrade and cannot while this connection is open. The module
        // deliberately keeps one connection for the life of the tab, so without this an old tab
        // blocks every new one indefinitely. Closing costs nothing: `open()` reconnects on the
        // next call, by which time the upgrade has run.
        db.onversionchange = () => { db.close(); dbPromise = null; };
        resolve(db);
      };
      req.onerror = () => { dbPromise = null; reject(req.error); };
      // The other half of the same problem, seen from the new tab: a still-open v1 connection
      // elsewhere fires `blocked` and then NEITHER `onsuccess` NOR `onerror`, so this promise
      // would never settle and every outbox call -- `all`, `putWithSeq`, `remove` -- would hang
      // for ever. An offline Save would hang inside `enqueue` too, so `createQueued` could not
      // even report the write as lost: the user gets a spinner and nothing else. With the
      // service worker on `autoUpdate`, a tab running the previous bundle is the expected state
      // for a while after every deploy, so this is the ordinary case, not a corner.
      req.onblocked = () => {
        dbPromise = null;
        reject(new Error('outbox: database upgrade blocked by another tab'));
      };
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
 * Writes one op, assigning `seq` when it does not have one yet -- reading the current maximum
 * and writing the new record in ONE readwrite transaction.
 *
 * That is what makes `seq` unique across tabs. IndexedDB serialises readwrite transactions over
 * the same object store, so two tabs cannot interleave a read-max with each other's write. The
 * counter this replaces was per tab, seeded once from its own separate transaction, so two tabs
 * opening the same queue at once handed out the SAME value -- and the queue's order then fell
 * back to whatever `getAll()` returned, which is the arbitrariness `seq` exists to remove.
 *
 * `Date.now()` is a floor, not just a seed: `queued_at` is epoch milliseconds and legacy records
 * are ordered by it, so starting past "now" keeps a freshly assigned `seq` strictly after every
 * such record even if the v2 backfill above never ran (a store that failed to upgrade, a record
 * written by something older). Two ops in the same millisecond still get distinct values,
 * because the second reads the first's `seq` back out of the index.
 */
function putWithSeq(op: QueuedOp): Promise<void> {
  return open().then((db) => new Promise<void>((resolve, reject) => {
    const tx = db.transaction(STORE, 'readwrite');
    const s = tx.objectStore(STORE);
    const row: QueuedOp = { queued_at: Date.now(), ...op };
    if (row.seq === undefined) {
      const cur = s.index(SEQ).openCursor(null, 'prev');
      cur.onsuccess = () => {
        const highest = (cur.result?.value as QueuedOp | undefined)?.seq ?? 0;
        s.put({ ...row, seq: Math.max(highest, Date.now()) + 1 });
      };
    } else {
      s.put(row);
    }
    tx.oncomplete = () => resolve();
    tx.onabort = () => reject(tx.error);
  }));
}

/**
 * Insertion order is preserved: keys are UUIDs, so `getAll()` returns them in key order, which
 * carries no relation to enqueue order -- and two ops enqueued in the same millisecond would
 * otherwise tie on `queued_at` too, leaving their relative order to that same meaningless key
 * order (MINOR 3). `seq`, assigned once per op by `putWithSeq` above and preserved on every
 * later rewrite of the same op (see `QueuedOp.seq`), is what actually breaks that tie
 * deterministically; `compareQueueOrder` falls back to `queued_at` only for a record written
 * before this field existed and never given one by the v2 backfill.
 */
export function idbStore(): OutboxStore {
  return {
    async all() {
      const rows = await run<QueuedOp[]>('readonly', (s) => s.getAll() as IDBRequest<QueuedOp[]>);
      return rows.sort(compareQueueOrder);
    },
    put: putWithSeq,
    async remove(id) { await run('readwrite', (s) => s.delete(id)); },
  };
}
