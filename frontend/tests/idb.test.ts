import { IDBFactory } from 'fake-indexeddb';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { QueuedOp } from '../src/lib/outbox';

/**
 * `idb.ts` is the only part of the outbox that talks to real storage, and it was the only part
 * with no tests at all -- everything else is exercised against `memoryStore`, which has none of
 * IndexedDB's semantics: no transactions, no `versionchange`, no `blocked`, no upgrades. Three
 * separate review findings lived in exactly that gap (a seed promise that poisoned itself, an
 * upgrade that could hang every tab, and the cross-tab `seq` collision itself), and none of
 * them were reachable from a test until now.
 *
 * The module caches its connection for the life of the tab, so each test re-imports it against
 * a fresh `IDBFactory` -- which is also what makes "another tab" expressible: a second import
 * over the same factory is a second connection to the same database.
 */
const DB = 'memto-outbox';
const STORE = 'ops';

async function freshModule() {
  globalThis.indexedDB = new IDBFactory();
  return loadModule();
}

async function loadModule() {
  // A fresh module instance each time, so the connection it caches for "the life of the tab"
  // does not leak between tests -- and so two calls give two independent connections, which is
  // what lets a test express "another tab".
  vi.resetModules();
  const mod = await import('../src/lib/idb');
  return mod.idbStore();
}

const op = (id: string, over: Partial<QueuedOp> = {}): QueuedOp => ({
  id, kind: 'activity.create', path: '/objects/1/activities', body: { title: id }, attempts: 0, ...over,
});

/** Opens the database directly, the way a second tab would. */
function rawOpen(version?: number): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = version === undefined ? indexedDB.open(DB) : indexedDB.open(DB, version);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
    req.onblocked = () => reject(new Error('blocked'));
  });
}

describe('idbStore', () => {
  beforeEach(() => {
    globalThis.indexedDB = new IDBFactory();
  });

  it('round-trips an op', async () => {
    const store = await freshModule();
    await store.put(op('a'));

    expect((await store.all()).map((o) => o.id)).toEqual(['a']);
  });

  it('removes an op', async () => {
    const store = await freshModule();
    await store.put(op('a'));
    await store.remove('a');

    expect(await store.all()).toEqual([]);
  });

  it('assigns an increasing seq and keeps the one an op already has', async () => {
    const store = await freshModule();
    await store.put(op('a'));
    await store.put(op('b'));
    const [first, second] = (await store.all()).map((o) => o.seq!);
    expect(second).toBeGreaterThan(first);

    // A rewrite of the same op -- what `replay` does when it records an attempt -- must not
    // renumber it, or a failed send would jump the op to the back of its own queue.
    const a = (await store.all()).find((o) => o.id === 'a')!;
    await store.put({ ...a, attempts: 1 });
    expect((await store.all()).find((o) => o.id === 'a')!.seq).toBe(first);
  });

  it('returns ops in queue order, not key order', async () => {
    const store = await freshModule();
    // Ids chosen so that alphabetical (which is what getAll returns) disagrees with insertion.
    await store.put(op('zzz'));
    await store.put(op('aaa'));

    expect((await store.all()).map((o) => o.id)).toEqual(['zzz', 'aaa']);
  });

  /**
   * The whole point of assigning `seq` inside the write transaction. Two connections to one
   * database, interleaved as tightly as the API allows: IndexedDB serialises readwrite
   * transactions over an object store, so no two ops can come out with the same number.
   */
  it('never hands the same seq to two connections writing at once', async () => {
    globalThis.indexedDB = new IDBFactory();
    const tabA = await loadModule();
    const tabB = await loadModule();

    await Promise.all([
      tabA.put(op('a1')), tabB.put(op('b1')),
      tabA.put(op('a2')), tabB.put(op('b2')),
      tabA.put(op('a3')), tabB.put(op('b3')),
    ]);

    const rows = await tabA.all();
    expect(rows).toHaveLength(6);
    const seqs = rows.map((o) => o.seq);
    expect(seqs.every((s) => typeof s === 'number')).toBe(true);
    expect(new Set(seqs).size).toBe(6);
    // Both connections read one queue, so they must agree on the order it is in.
    expect(await tabB.all()).toEqual(rows);
  });

  it('gives a fresh op a seq past a legacy record that only has queued_at', async () => {
    globalThis.indexedDB = new IDBFactory();
    const store = await loadModule();
    await store.put(op('anchor')); // creates the database at v2

    const db = await rawOpen();
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(STORE, 'readwrite');
      tx.objectStore(STORE).put({ ...op('legacy'), queued_at: Date.now() + 60_000, seq: undefined });
      tx.oncomplete = () => resolve();
      tx.onabort = () => reject(tx.error);
    });
    db.close();

    await store.put(op('fresh'));
    const rows = await store.all();
    const legacy = rows.find((o) => o.id === 'legacy')!;
    const fresh = rows.find((o) => o.id === 'fresh')!;
    expect(legacy.seq).toBeUndefined();
    // `Date.now()` is a floor on an assigned seq precisely so this holds without the record
    // having to be rewritten.
    expect(fresh.seq!).toBeGreaterThan(legacy.queued_at! - 60_000);
  });

  describe('upgrading a v1 database', () => {
    /** The store as it was before `seq` had an index: no index, records without `seq`. */
    async function seedV1(rows: Array<Partial<QueuedOp> & { id: string }>) {
      const db = await new Promise<IDBDatabase>((resolve, reject) => {
        const req = indexedDB.open(DB, 1);
        req.onupgradeneeded = () => req.result.createObjectStore(STORE, { keyPath: 'id' });
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => reject(req.error);
      });
      await new Promise<void>((resolve, reject) => {
        const tx = db.transaction(STORE, 'readwrite');
        for (const r of rows) tx.objectStore(STORE).put(r);
        tx.oncomplete = () => resolve();
        tx.onabort = () => reject(tx.error);
      });
      db.close();
    }

    it('keeps every record and gives each one a seq from its queued_at', async () => {
      globalThis.indexedDB = new IDBFactory();
      await seedV1([
        { ...op('older'), queued_at: 1_700_000_000_000 },
        { ...op('newer'), queued_at: 1_700_000_005_000 },
      ]);

      const store = await loadModule();
      const rows = await store.all();

      expect(rows.map((o) => o.id)).toEqual(['older', 'newer']);
      expect(rows.map((o) => o.seq)).toEqual([1_700_000_000_000, 1_700_000_005_000]);
    });

    it('carries a queued upload\'s blob through the upgrade', async () => {
      globalThis.indexedDB = new IDBFactory();
      const blob = new Blob(['a photo']);
      await seedV1([{ ...op('upload'), kind: 'attachment.upload', blob, filename: 'p.png', queued_at: 1 }]);

      const store = await loadModule();
      const [row] = await store.all();

      expect(row.filename).toBe('p.png');
      expect(await row.blob!.text()).toBe('a photo');
    });

    it('leaves an already-upgraded database alone', async () => {
      globalThis.indexedDB = new IDBFactory();
      const first = await loadModule();
      await first.put(op('a'));
      const before = await first.all();

      const second = await loadModule();
      expect(await second.all()).toEqual(before);
    });
  });

  /**
   * The module holds one connection for the life of the tab, so a tab on the previous bundle
   * would block a new tab's upgrade indefinitely: `open` fires `blocked` and then neither
   * `onsuccess` nor `onerror`, leaving every outbox call hanging -- an offline Save hangs inside
   * `enqueue`, so the write cannot even be reported as lost. With the service worker on
   * `autoUpdate`, a tab running the old bundle is the expected state after every deploy.
   */
  it('closes its connection so another tab can upgrade, instead of blocking it', async () => {
    globalThis.indexedDB = new IDBFactory();
    const store = await loadModule();
    await store.put(op('a')); // opens and caches the connection

    // `rawOpen` rejects on `blocked`, so this only resolves because the held connection closed.
    const upgraded = await rawOpen(3);
    expect(upgraded.version).toBe(3);
    upgraded.close();
  });

  /**
   * What happens to the old tab afterwards: its next call reconnects at the version its own
   * bundle knows, which the database has now moved past. That is a `VersionError` -- an
   * immediate, reported failure rather than a hang, which is the whole point. The old bundle
   * could not read the new schema in any case; the service worker reloads it into the new one.
   */
  it('fails fast, rather than hanging, once the schema has moved past it', async () => {
    globalThis.indexedDB = new IDBFactory();
    const store = await loadModule();
    await store.put(op('a'));

    const upgraded = await rawOpen(3);
    upgraded.close();

    await expect(store.all()).rejects.toBeTruthy();
  });
});
