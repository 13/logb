import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createObjectQueued, createQueued, createReminderQueued, setOutboxStoreForTesting, updateQueued, uploadQueued } from '../src/lib/api-outbox';
import { SAVE_TIMEOUT_MS, setOutboxUser } from '../src/lib/api';
import { memoryStore, type OutboxStore } from '../src/lib/outbox';

/**
 * A save that never gets an answer -- one bar of signal, a captive portal that swallows the
 * request -- used to hold the form on "saving" for as long as the network cared to take. It must
 * give up after `SAVE_TIMEOUT_MS` and fall back to the outbox exactly as it does offline.
 */
describe('a save the network never answers', () => {
  let store: OutboxStore;
  let timers: AbortController[];
  let bodies: Array<Record<string, unknown>>;

  beforeEach(() => {
    setOutboxUser(1);
    store = memoryStore();
    setOutboxStoreForTesting(store);
    timers = [];
    bodies = [];
    // Stands in for the deadline: the test decides when it has passed, instead of waiting it out.
    vi.spyOn(AbortSignal, 'timeout').mockImplementation((ms: number) => {
      expect(ms).toBe(SAVE_TIMEOUT_MS);
      const c = new AbortController();
      timers.push(c);
      return c.signal;
    });
    // A request that hangs until it is aborted, and only then fails.
    globalThis.fetch = vi.fn((_url: string, init?: RequestInit) => {
      if (typeof init?.body === 'string') bodies.push(JSON.parse(init.body));
      return new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener('abort', () => reject(init.signal!.reason));
      });
    }) as unknown as typeof fetch;
  });

  afterEach(() => vi.restoreAllMocks());

  async function expire(): Promise<void> {
    await new Promise((r) => setTimeout(r, 0));
    expect(timers).toHaveLength(1);
    timers[0].abort(new DOMException('signal timed out', 'TimeoutError'));
  }

  it('queues an activity create, under the op id the timed-out attempt sent', async () => {
    const saving = createQueued('/objects/1/activities', { title: 'x' }, -7);
    await expire();
    expect(await saving).toBeNull();
    const [op] = await store.all();
    expect(op).toMatchObject({ kind: 'activity.create', tempId: -7 });
    // The server may have received the timed-out request after all; the replay carries the same
    // id, which the server answers with the row it already made rather than a second one.
    expect(op.id).toBe(bodies[0].client_op_id);
  });

  it('queues an activity edit', async () => {
    const saving = updateQueued('/activities/5', { title: 'x' });
    await expire();
    expect(await saving).toBe(false);
    expect((await store.all())[0]).toMatchObject({ kind: 'activity.update', path: '/activities/5' });
  });

  it('stamps the edit it gives up on, so a late landing cannot beat a newer one', async () => {
    // The abandoned PATCH may still land -- after the user's next edit, even. Without a stamp the
    // server takes it as made the moment it arrives and it overwrites that newer edit.
    const saving = updateQueued('/activities/5', { title: 'x' });
    await expire();
    await saving;
    expect(typeof bodies[0].edited_at).toBe('string');
    expect((await store.all())[0].body.edited_at).toBe(bodies[0].edited_at);
  });

  it('queues an object create, under the client_uuid the timed-out attempt sent', async () => {
    const saving = createObjectQueued({ name: 'x' }, -3);
    await expire();
    expect(await saving).toBeNull();
    const [op] = await store.all();
    expect(op.kind).toBe('object.create');
    expect(op.id).toBe(bodies[0].client_uuid);
  });

  it('queues a reminder create, under the client_uuid the timed-out attempt sent', async () => {
    const saving = createReminderQueued('/objects/1/reminders', { title: 'x' });
    await expire();
    expect(await saving).toBeNull();
    const [op] = await store.all();
    expect(op.kind).toBe('reminder.create');
    expect(op.id).toBe(bodies[0].client_uuid);
  });

  it('gives an upload no such deadline: a large photo on a slow link can take longer', async () => {
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      expect(init?.signal).toBeUndefined();
      return { ok: true, status: 201, headers: { get: () => 'application/json' }, json: async () => ({ id: 1 }) } as unknown as Response;
    }) as unknown as typeof fetch;
    await uploadQueued('/objects/1/attachments', new Blob(['x']), 'x.jpg', 5);
    expect(globalThis.fetch).toHaveBeenCalledTimes(1);
    expect(AbortSignal.timeout).not.toHaveBeenCalled();
  });
});
