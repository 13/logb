import { describe, expect, it, vi } from 'vitest';
import { memoryStore, enqueue, replay, pendingCount, serialize, type QueuedOp } from '../src/lib/outbox';
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

// `flushOutbox` (./api.ts) wraps `doFlushOutbox` in this so overlapping triggers -- startup,
// `online`, `visibilitychange`, a manual retry -- can't run two concurrent passes over the same
// outbox snapshot. That race is what resurrects a just-completed op as a phantom pending row
// (see the comment on `serialize`), so the guard itself gets its own coverage independent of
// any particular caller.
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
