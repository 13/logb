import { afterEach, describe, expect, it, vi } from 'vitest';
import { compareQueueOrder, createLock, memoryStore, newOpId, enqueue, removeQueuedActivity, replay, pendingCount, serialize, updateQueuedActivityBody, type QueuedOp } from '../src/lib/outbox';
import { ApiError } from '../src/lib/api-error';

const op = (id: string, over: Partial<QueuedOp> = {}): QueuedOp =>
  ({ id, kind: 'activity.create', path: '/objects/1/activities', body: {}, attempts: 0, ...over });

describe('outbox', () => {
  it('replays in the order the ops were queued', async () => {
    const store = memoryStore();
    await enqueue(store, op('a'));
    await enqueue(store, op('b'));
    const seen: string[] = [];
    await replay(store, async (o) => { seen.push(o.id); return { id: 1 }; });
    expect(seen).toEqual(['a', 'b']);
    expect(await pendingCount(store)).toBe(0);
  });

  it('rewrites a temp activity id into a later upload once the create lands', async () => {
    const store = memoryStore();
    await enqueue(store, op('create', { tempId: -1 }));
    await enqueue(store, op('upload', { kind: 'attachment.upload', path: '/objects/1/attachments', body: { activity_id: -1 } }));
    const sent: unknown[] = [];
    await replay(store, async (o) => { sent.push(structuredClone(o.body)); return { id: 42 }; });
    expect(sent[1]).toEqual({ activity_id: 42 });
  });

  it('a later pass still sends the real id, not the temp one, when the create resolved in an earlier pass that stopped before the dependent upload was sent', async () => {
    const store = memoryStore();
    await enqueue(store, op('create', { tempId: -1 }));
    await enqueue(store, op('blocker'));
    await enqueue(store, op('upload', { kind: 'attachment.upload', path: '/objects/1/attachments', body: { activity_id: -1 } }));

    // Pass 1: the create resolves -1 -> 42, but 'blocker' then fails on a non-4xx error, so
    // `replay` stops right there -- 'upload' is never even attempted in this pass, and pass
    // 1's in-memory `resolved` map is discarded the moment `replay` returns.
    await replay(store, async (o) => {
      if (o.id === 'create') return { id: 42 };
      if (o.id === 'blocker') throw new Error('offline');
      throw new Error('must not reach "upload" in pass 1 -- "blocker" should have stopped the pass first');
    });

    const afterPass1 = await store.all();
    expect(afterPass1.find((r) => r.id === 'create')).toBeUndefined(); // the create is done
    expect(afterPass1.find((r) => r.id === 'upload')?.body.activity_id).toBe(42); // persisted already

    // Pass 2: a brand-new call, so a brand-new, empty `resolved` map -- the real id can only
    // reach the request if it was written into the STORED op, which is exactly what this test
    // pins down.
    const sent: unknown[] = [];
    await replay(store, async (o) => {
      sent.push({ id: o.id, body: structuredClone(o.body) });
      if (o.id === 'blocker') return { id: 1 };
      return { id: 1 };
    });

    const uploadSent = sent.find((s) => (s as { id: string }).id === 'upload');
    expect(uploadSent).toEqual({ id: 'upload', body: { activity_id: 42 } });
    expect(await pendingCount(store)).toBe(0);
  });

  it('keeps the id resolved earlier in the SAME pass when the dependent upload itself then fails retryably', async () => {
    const store = memoryStore();
    await enqueue(store, op('create', { tempId: -1 }));
    await enqueue(store, op('upload', { kind: 'attachment.upload', path: '/objects/1/attachments', body: { activity_id: -1 } }));

    // Pass 1: the create succeeds, so `persistResolvedId` writes `activity_id: 42` into
    // 'upload's stored body -- but the upload's OWN send then throws a non-4xx (a dropped
    // connection mid-multipart, by far the likeliest op to fail, being the megabyte one).
    // Before the fix, the catch wrote back `{ ...op, attempts }` from the top-of-pass snapshot
    // taken before the create even ran -- i.e. `activity_id: -1` -- discarding the 42 that had
    // just been persisted moments earlier in this exact pass.
    await replay(store, async (o) => {
      if (o.id === 'create') return { id: 42 };
      if (o.id === 'upload') throw new Error('dropped mid-multipart');
      throw new Error(`unexpected op ${o.id}`);
    });

    const afterPass1 = await store.all();
    expect(afterPass1.find((r) => r.id === 'create')).toBeUndefined(); // the create is done
    const upload = afterPass1.find((r) => r.id === 'upload');
    expect(upload?.body.activity_id).toBe(42); // the resolved id must survive the retryable failure
    expect(upload?.dead).toBeFalsy();
    expect(upload?.attempts).toBe(1);

    // Pass 2: a brand-new call, so a brand-new, empty `resolved` map -- the real id can only
    // reach this request if pass 1 preserved it in the STORE, which is exactly what this pins
    // down. Before the fix this sent -1, which the server would 404 (no such activity),
    // parking the op dead forever with no way for the photo to ever be sent.
    const sent: unknown[] = [];
    await replay(store, async (o) => {
      sent.push({ id: o.id, body: structuredClone(o.body) });
      return { id: 1 };
    });
    expect(sent).toEqual([{ id: 'upload', body: { activity_id: 42 } }]);
    expect(await pendingCount(store)).toBe(0);
  });

  it('keeps an op queued when the send fails', async () => {
    const store = memoryStore();
    await enqueue(store, op('a'));
    await replay(store, async () => { throw new Error('offline'); });
    expect(await pendingCount(store)).toBe(1);
  });

  it('marks an op dead after three failures instead of retrying it forever', async () => {
    const store = memoryStore();
    await enqueue(store, op('a'));
    for (let i = 0; i < 3; i++) await replay(store, async () => { throw new Error('offline'); });
    const all = await store.all();
    expect(all[0].dead).toBe(true);
    expect(await pendingCount(store)).toBe(0);
  });

  it('does not replay a dead op', async () => {
    const store = memoryStore();
    await enqueue(store, op('a', { dead: true, attempts: 3 }));
    let calls = 0;
    await replay(store, async () => { calls++; return { id: 1 }; });
    expect(calls).toBe(0);
  });

  it('parks a 4xx op as dead immediately, without retrying it', async () => {
    const store = memoryStore();
    await enqueue(store, op('a'));
    await replay(store, async () => { throw new ApiError(422, 'invalid', 'nope'); });
    const [row] = await store.all();
    expect(row.dead).toBe(true);
    expect(row.attempts).toBe(0); // never incremented -- it was never going to be retried
  });

  it('does not let a doomed (4xx) op block a healthy op behind it in the same pass', async () => {
    const store = memoryStore();
    await enqueue(store, op('doomed'));
    await enqueue(store, op('healthy'));
    const sent: string[] = [];
    await replay(store, async (o) => {
      sent.push(o.id);
      if (o.id === 'doomed') throw new ApiError(400, 'bad_request', 'nope');
      return { id: 1 };
    });
    expect(sent).toEqual(['doomed', 'healthy']);
    const all = await store.all();
    expect(all).toHaveLength(1);
    expect(all[0].id).toBe('doomed');
    expect(all[0].dead).toBe(true);
    expect(await pendingCount(store)).toBe(0); // the healthy op was sent and removed
  });

  it('still stops the pass on a non-4xx failure, unlike a 4xx rejection', async () => {
    const store = memoryStore();
    await enqueue(store, op('a'));
    await enqueue(store, op('b'));
    const sent: string[] = [];
    await replay(store, async (o) => {
      sent.push(o.id);
      throw new Error('offline');
    });
    expect(sent).toEqual(['a']); // 'b' never attempted -- ordering must hold
    const all = await store.all();
    expect(all).toHaveLength(2);
    expect(all.find((r) => r.id === 'a')?.dead).toBeFalsy();
  });
});

/**
 * `removeQueuedActivity` is what `cancel()` in ActivityForm.svelte calls instead of DELETE
 * when the draft it is discarding was never more than a queued `activity.create` -- there is
 * no server row to delete, and leaving the create (or an upload still naming its temp id)
 * behind would replay it later, creating exactly the stray entry the cancel was meant to
 * prevent.
 */
describe('removeQueuedActivity', () => {
  it('removes the queued create identified by its tempId', async () => {
    const store = memoryStore();
    await enqueue(store, op('create', { tempId: -1 }));
    await removeQueuedActivity(store, -1);
    expect(await store.all()).toHaveLength(0);
  });

  it('also removes a dependent upload that still names the tempId', async () => {
    const store = memoryStore();
    await enqueue(store, op('create', { tempId: -1 }));
    await enqueue(store, op('upload', { kind: 'attachment.upload', path: '/objects/1/attachments', body: { activity_id: -1 } }));
    await removeQueuedActivity(store, -1);
    expect(await store.all()).toHaveLength(0);
  });

  it('leaves ops for a different draft untouched', async () => {
    const store = memoryStore();
    await enqueue(store, op('create', { tempId: -1 }));
    await enqueue(store, op('other-create', { tempId: -2 }));
    await enqueue(store, op('other-upload', { kind: 'attachment.upload', path: '/objects/1/attachments', body: { activity_id: -2 } }));
    await removeQueuedActivity(store, -1);
    const remaining = (await store.all()).map((o) => o.id).sort();
    expect(remaining).toEqual(['other-create', 'other-upload']);
  });

  it('is a no-op when nothing matches the tempId', async () => {
    const store = memoryStore();
    await enqueue(store, op('unrelated'));
    await removeQueuedActivity(store, -99);
    expect(await store.all()).toHaveLength(1);
  });
});

/**
 * `updateQueuedActivityBody` is what a Save made before the queued create it belongs to has
 * even reached the server folds into that same op, since the outbox has no "edit" op kind and
 * a PATCH would just fail against a server row that does not exist yet.
 */
describe('updateQueuedActivityBody', () => {
  it('overwrites the body of the queued create identified by its tempId', async () => {
    const store = memoryStore();
    await enqueue(store, op('create', { tempId: -1, body: { title: 'first' } }));
    await updateQueuedActivityBody(store, -1, { title: 'second' });
    const [row] = await store.all();
    expect(row.body).toEqual({ title: 'second' });
    expect(row.id).toBe('create'); // same op, same client_op_id -- not a second create
  });

  it('leaves a different op untouched', async () => {
    const store = memoryStore();
    await enqueue(store, op('create', { tempId: -1, body: { title: 'first' } }));
    await enqueue(store, op('other', { tempId: -2, body: { title: 'other' } }));
    await updateQueuedActivityBody(store, -1, { title: 'second' });
    const other = (await store.all()).find((o) => o.id === 'other');
    expect(other?.body).toEqual({ title: 'other' });
  });

  it('is a no-op when no live op has this tempId', async () => {
    const store = memoryStore();
    await enqueue(store, op('dead', { tempId: -1, dead: true, body: { title: 'first' } }));
    await updateQueuedActivityBody(store, -1, { title: 'second' });
    const [row] = await store.all();
    expect(row.body).toEqual({ title: 'first' }); // the dead op was not revived or rewritten
  });
});

// `flushOutbox` (./api.ts) wraps `doFlushOutbox` in this so overlapping triggers -- startup,
// `online`, `visibilitychange`, a manual retry -- can't run two concurrent passes over the same
// outbox snapshot. That race is what resurrects a just-completed op as a phantom pending row
// (see the comment on `serialize`), so the guard itself gets its own coverage independent of
// any particular caller.
/**
 * MINOR 3: `idbStore.all()` (`./idb.ts`) sorted on `queued_at ?? 0` alone, and two ops
 * enqueued in the same millisecond tie on that -- leaving their relative order to fall out of
 * `IDBObjectStore.getAll()`'s key (UUID) order, which carries no relation to enqueue order at
 * all. `seq` breaks that tie deterministically; `compareQueueOrder` is the ordering `idbStore`
 * actually sorts by.
 */
describe('compareQueueOrder', () => {
  it('orders by seq when both ops have one', () => {
    const a = op('a', { seq: 5 });
    const b = op('b', { seq: 2 });
    expect(compareQueueOrder(a, b)).toBeGreaterThan(0);
    expect(compareQueueOrder(b, a)).toBeLessThan(0);
  });

  it('falls back to queued_at for a record written before seq existed', () => {
    const a = op('a', { queued_at: 200 });
    const b = op('b', { queued_at: 100 });
    expect(compareQueueOrder(a, b)).toBeGreaterThan(0);
    expect(compareQueueOrder(b, a)).toBeLessThan(0);
  });

  it('breaks a tie that queued_at alone cannot: two ops enqueued in the same millisecond', () => {
    const a = op('a', { queued_at: 1000, seq: 1 });
    const b = op('b', { queued_at: 1000, seq: 2 });
    expect(compareQueueOrder(a, b)).toBeLessThan(0);
    expect(compareQueueOrder(b, a)).toBeGreaterThan(0);
  });

  /// Two ops carrying no ordering information at all are ordered by id -- not "tied", which
  /// left them to the stability of whatever array `sort` was handed. For records like these
  /// (written before `seq` existed, so read back from IndexedDB in UUID key order) id order IS
  /// that order, so nothing about the queue changes; it is now decided rather than inherited.
  it('orders two ops with neither field by id, consistently in both directions', () => {
    expect(compareQueueOrder(op('a'), op('b'))).toBeLessThan(0);
    expect(compareQueueOrder(op('b'), op('a'))).toBeGreaterThan(0);
    expect(compareQueueOrder(op('a'), op('a'))).toBe(0);
  });
});

/**
 * IMPORTANT 1 relies on this mutex to keep a replay pass and a UI write (`updateQueuedActivity`
 * / `cancelQueuedActivity` in `./api.ts`) from ever touching the outbox store at the same time.
 */
describe('createLock', () => {
  it('runs callers one at a time, in the order run() was called -- not the order their work finishes', async () => {
    const lock = createLock();
    const order: string[] = [];
    let releaseFirst!: () => void;
    const first = lock.run(() => new Promise<void>((resolve) => {
      order.push('first-start');
      releaseFirst = () => { order.push('first-end'); resolve(); };
    }));
    const second = lock.run(async () => { order.push('second'); });

    await new Promise((r) => setTimeout(r, 0));
    expect(order).toEqual(['first-start']); // second must not have started yet

    releaseFirst();
    await Promise.all([first, second]);
    expect(order).toEqual(['first-start', 'first-end', 'second']);
  });

  it('still runs a later caller after an earlier one throws', async () => {
    const lock = createLock();
    await expect(lock.run(async () => { throw new Error('boom'); })).rejects.toThrow('boom');
    await expect(lock.run(async () => 42)).resolves.toBe(42);
  });

  it('serves a caller already waiting before one requested later, however many arrive in between', async () => {
    // This is what stops a UI write from being starved by back-to-back flush passes: once it
    // has started waiting for the lock, no later-requested run can cut in front of it.
    const lock = createLock();
    const order: string[] = [];
    let releaseFirst!: () => void;
    lock.run(() => new Promise<void>((resolve) => { releaseFirst = resolve; }));

    const waiting = lock.run(async () => { order.push('waiting'); });
    lock.run(async () => { order.push('late'); });

    await new Promise((r) => setTimeout(r, 0)); // let run()'s executor actually assign releaseFirst
    releaseFirst();
    await waiting;
    expect(order[0]).toBe('waiting');
  });
});

describe('serialize', () => {
  it('shares one in-flight run across calls that overlap it', async () => {
    let calls = 0;
    let release!: () => void;
    const wrapped = serialize(() => new Promise<void>((resolve) => {
      calls++;
      release = resolve;
    }));

    const first = wrapped();
    const second = wrapped();

    expect(calls).toBe(1); // the second call did not start a fresh run
    expect(second).toBe(first); // both callers are handed the exact same promise

    release();
    await Promise.all([first, second]);
  });

  it('starts a genuinely new run once the previous one has settled', async () => {
    const fn = vi.fn(async () => {});
    const wrapped = serialize(fn);

    await wrapped();
    await wrapped();

    expect(fn).toHaveBeenCalledTimes(2);
  });

  it('lets the next call start a new run even after the previous one rejected', async () => {
    const fn = vi.fn()
      .mockRejectedValueOnce(new Error('boom'))
      .mockResolvedValueOnce(undefined);
    const wrapped = serialize(fn);

    await expect(wrapped()).rejects.toThrow('boom');
    await expect(wrapped()).resolves.toBeUndefined();
    expect(fn).toHaveBeenCalledTimes(2);
  });

  it('resolves every caller of a shared run to the same value', async () => {
    const wrapped = serialize(async () => 42);
    const [a, b] = await Promise.all([wrapped(), wrapped()]);
    expect(a).toBe(42);
    expect(b).toBe(42);
  });
});

/**
 * The mutex above only excludes callers inside ONE tab. The outbox store it guards is
 * IndexedDB, which every same-origin tab shares, so a second tab running its own replay pass
 * is exactly the interleaving IMPORTANT 1 closed within a tab -- one tab's pass reading an op,
 * awaiting its `send()`, and writing the result back over what the other tab did in between.
 * A cross-tab lock (the Web Locks API in the browser, a fake here) is what makes the exclusion
 * hold across tabs; these tests drive two separate lock instances, standing in for two tabs,
 * against one shared manager.
 */
function fakeLockManager() {
  const tails = new Map<string, Promise<void>>();
  const held: string[] = [];
  return {
    held,
    request: <T>(name: string, fn: () => Promise<T>): Promise<T> => {
      const started = (tails.get(name) ?? Promise.resolve()).then(fn, fn);
      tails.set(name, started.then(() => undefined, () => undefined));
      return started;
    },
  };
}

describe('createLock across tabs', () => {
  it('never lets two tabs sharing one lock name run at the same time', async () => {
    const manager = fakeLockManager();
    const tabA = createLock('logb-outbox', manager);
    const tabB = createLock('logb-outbox', manager);
    const order: string[] = [];
    let releaseA!: () => void;

    const a = tabA.run(() => new Promise<void>((resolve) => {
      order.push('A-start');
      releaseA = () => { order.push('A-end'); resolve(); };
    }));
    const b = tabB.run(async () => { order.push('B'); });

    await new Promise((r) => setTimeout(r, 0));
    expect(order).toEqual(['A-start']); // the other tab must not have started yet

    releaseA();
    await Promise.all([a, b]);
    expect(order).toEqual(['A-start', 'A-end', 'B']);
  });

  it('runs the callback anyway on a browser with no Web Locks', async () => {
    // Safari before 15.4, and any non-secure context. Losing cross-tab exclusion there is the
    // status quo; losing the replay pass entirely would not be.
    const lock = createLock('logb-outbox', undefined);
    await expect(lock.run(async () => 42)).resolves.toBe(42);
  });

  it('runs the callback anyway when acquiring the cross-tab lock itself fails', async () => {
    let calls = 0;
    const lock = createLock('logb-outbox', {
      request: async () => { throw new Error('lock manager unavailable'); },
    });
    await expect(lock.run(async () => { calls++; return 42; })).resolves.toBe(42);
    expect(calls).toBe(1);
  });

  it('does not retry the callback when the callback is what failed', async () => {
    // The fallback above must key on the lock manager failing, not on any rejection: a send
    // that threw has already been attempted, and running it a second time would replay it.
    const manager = fakeLockManager();
    const lock = createLock('logb-outbox', manager);
    let calls = 0;
    await expect(lock.run(async () => { calls++; throw new Error('boom'); })).rejects.toThrow('boom');
    expect(calls).toBe(1);
  });

  it('keeps its own FIFO order across the cross-tab hop', async () => {
    const manager = fakeLockManager();
    const lock = createLock('logb-outbox', manager);
    const order: string[] = [];
    let releaseFirst!: () => void;
    lock.run(() => new Promise<void>((resolve) => { releaseFirst = resolve; }));

    const waiting = lock.run(async () => { order.push('waiting'); });
    lock.run(async () => { order.push('late'); });

    await new Promise((r) => setTimeout(r, 0));
    releaseFirst();
    await waiting;
    expect(order[0]).toBe('waiting');
  });
});

describe('compareQueueOrder with a cross-tab seq collision', () => {
  it('falls back to when the op was queued, then to its id', () => {
    const earlier = op('zzz', { seq: 7, queued_at: 1_000 });
    const later = op('aaa', { seq: 7, queued_at: 2_000 });
    expect([later, earlier].sort(compareQueueOrder).map((o) => o.id)).toEqual(['zzz', 'aaa']);

    // Same millisecond too: no true order exists, but every tab must at least agree on one.
    const a = op('aaa', { seq: 7, queued_at: 1_000 });
    const b = op('bbb', { seq: 7, queued_at: 1_000 });
    expect([b, a].sort(compareQueueOrder).map((o) => o.id)).toEqual(['aaa', 'bbb']);
    expect([a, b].sort(compareQueueOrder).map((o) => o.id)).toEqual(['aaa', 'bbb']);
  });
});

/**
 * Self-hosting means reaching this over the LAN as `http://192.168.x.x:8080` at least some of
 * the time, and a plain-http origin is not a secure context: Chrome does not expose
 * `crypto.randomUUID` there at all. `newOpId` runs at the very top of `createQueued`, before
 * the request and outside its try, so logging an activity from a phone on the LAN threw a
 * TypeError and failed outright -- the ordinary save, not an offline corner.
 */
describe('newOpId on an origin without crypto.randomUUID', () => {
  const real = globalThis.crypto;
  afterEach(() => { Object.defineProperty(globalThis, 'crypto', { value: real, configurable: true }); });

  function withoutRandomUUID() {
    Object.defineProperty(globalThis, 'crypto', {
      value: { getRandomValues: (a: Uint8Array<ArrayBuffer>) => real.getRandomValues(a) },
      configurable: true,
    });
  }

  it('still returns a well-formed v4 uuid', () => {
    withoutRandomUUID();
    const id = newOpId();

    expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  });

  it('returns a different id every time', () => {
    withoutRandomUUID();
    const ids = new Set(Array.from({ length: 500 }, () => newOpId()));

    // Two devices queueing offline must not collide: the server would read one write as a
    // replay of the other and silently drop it.
    expect(ids.size).toBe(500);
  });

  it('uses the platform implementation when there is one', () => {
    const spy = vi.spyOn(real, 'randomUUID');
    newOpId();
    expect(spy).toHaveBeenCalled();
    spy.mockRestore();
  });
});
