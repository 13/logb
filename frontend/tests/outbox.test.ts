import { describe, expect, it } from 'vitest';
import { memoryStore, enqueue, replay, pendingCount, type QueuedOp } from '../src/lib/outbox';
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
