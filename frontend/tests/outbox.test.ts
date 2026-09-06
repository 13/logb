import { describe, expect, it, vi } from 'vitest';
import { compareQueueOrder, createLock, memoryStore, enqueue, removeQueuedActivity, replay, pendingCount, serialize, updateQueuedActivityBody, type QueuedOp } from '../src/lib/outbox';
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

  it('treats two ops with neither field as tied', () => {
    expect(compareQueueOrder(op('a'), op('b'))).toBe(0);
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
