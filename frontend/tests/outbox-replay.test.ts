import { describe, it, expect, beforeEach, vi } from 'vitest';
import { cancelQueuedActivity, createQueued, flushOutbox, onOutboxFlushed, retryDead, setOutboxStoreForTesting, updateQueuedActivity, uploadQueued, ApiError, outboxDeadCount } from '../src/lib/api';
import { memoryStore, enqueue } from '../src/lib/outbox';

function jsonResponse(status: number, body: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: (k: string) => (k.toLowerCase() === 'content-type' ? 'application/json' : null) },
    json: async () => body,
    text: async () => JSON.stringify(body),
  } as unknown as Response;
}

/**
 * The offline outbox's whole no-duplicate guarantee rests on one property: the `client_op_id`
 * sent when a write is first attempted is the SAME id sent when it is later replayed (see the
 * comment on `createQueued` in `../src/lib/api.ts`). `createQueued` and `flushOutbox` have no
 * unit coverage of their own -- only `isRejection`, which they depend on, is covered here -- so
 * this property was pinned only by the (much slower, much less precise) Playwright run. This
 * test drives both functions directly, against a `memoryStore()` this test controls, and would
 * fail immediately if a future change ever minted a fresh id for the replay.
 */
describe('createQueued + flushOutbox', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('replays a queued write with the exact client_op_id the first attempt sent', async () => {
    const calls: Array<Record<string, unknown>> = [];
    let attempt = 0;
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      attempt++;
      calls.push(JSON.parse(init?.body as string));
      if (attempt === 1) {
        // A dropped connection: fetch itself never gets an answer to reject or resolve with,
        // which is exactly what `isRejection` treats as "unknown, so queue it" rather than a
        // server refusal.
        throw new TypeError('Failed to fetch');
      }
      return {
        ok: true,
        status: 201,
        headers: { get: (k: string) => (k.toLowerCase() === 'content-type' ? 'application/json' : null) },
        json: async () => ({ id: 42 }),
        text: async () => '{"id":42}',
      } as unknown as Response;
    }) as unknown as typeof fetch;

    const result = await createQueued('/objects/1/activities', { date: '2024-01-01', category: 'fuel', title: 'x' });
    expect(result).toBeNull(); // queued, not sent successfully

    expect(calls).toHaveLength(1);
    const firstOpId = calls[0].client_op_id;
    expect(typeof firstOpId).toBe('string');

    await flushOutbox();

    expect(calls).toHaveLength(2);
    const replayedOpId = calls[1].client_op_id;
    expect(replayedOpId).toBe(firstOpId);
  });
});

/**
 * The `attachment.upload` counterpart to the suite above: `uploadQueued` must send multipart
 * form data (not JSON), and `flushOutbox` must be able to resend that same multipart request
 * later with the exact file, filename and `client_op_id` the first attempt used.
 */
describe('uploadQueued + flushOutbox', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('sends a real activity id immediately as multipart form data', async () => {
    const calls: FormData[] = [];
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      calls.push(init?.body as FormData);
      return jsonResponse(201, { id: 7 });
    }) as unknown as typeof fetch;

    const file = new File(['abc'], 'photo.jpg', { type: 'image/jpeg' });
    const result = await uploadQueued<{ id: number }>('/objects/1/attachments', file, 'photo.jpg', 9);

    expect(result).toEqual({ id: 7 });
    expect(calls).toHaveLength(1);
    const form = calls[0];
    expect((form.get('file') as File).name).toBe('photo.jpg');
    expect(form.get('activity_id')).toBe('9');
    expect(typeof form.get('client_op_id')).toBe('string');
  });

  it('queues the upload on a dropped connection and replays it with the same client_op_id, file and filename', async () => {
    const calls: FormData[] = [];
    let attempt = 0;
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      attempt++;
      calls.push(init?.body as FormData);
      if (attempt === 1) throw new TypeError('Failed to fetch'); // dropped connection
      return jsonResponse(201, { id: 8 });
    }) as unknown as typeof fetch;

    const file = new File(['abc'], 'receipt.pdf', { type: 'application/pdf' });
    const result = await uploadQueued<{ id: number }>('/objects/1/attachments', file, 'receipt.pdf', 3);
    expect(result).toBeNull(); // queued, not sent successfully
    expect(calls).toHaveLength(1);
    const firstOpId = calls[0].get('client_op_id');
    expect(typeof firstOpId).toBe('string');

    await flushOutbox();

    expect(calls).toHaveLength(2);
    expect(calls[1].get('client_op_id')).toBe(firstOpId);
    expect((calls[1].get('file') as File).name).toBe('receipt.pdf');
    expect(calls[1].get('activity_id')).toBe('3');
  });

  it('skips the immediate attempt and queues straight away for a still-unresolved (negative) temp activity id', async () => {
    const calls: unknown[] = [];
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      calls.push(init);
      return jsonResponse(201, { id: 99 });
    }) as unknown as typeof fetch;

    const file = new File(['abc'], 'photo.jpg', { type: 'image/jpeg' });
    // -1 stands for a parent `activity.create` that is itself still only queued -- the server
    // has never heard of that activity, so sending now would 404 rather than queue.
    const result = await uploadQueued<{ id: number }>('/objects/1/attachments', file, 'photo.jpg', -1);

    expect(result).toBeNull();
    expect(calls).toHaveLength(0); // fetch was never even attempted
  });

  it('rethrows a genuine server rejection instead of queuing it', async () => {
    globalThis.fetch = vi.fn(async () => jsonResponse(422, { error: 'invalid', message: 'bad file' })) as unknown as typeof fetch;
    const file = new File(['abc'], 'photo.exe', { type: 'application/octet-stream' });
    await expect(uploadQueued('/objects/1/attachments', file, 'photo.exe', 1)).rejects.toBeInstanceOf(ApiError);
  });
});

/**
 * A negative `activity_id` reaching the network at all means a guaranteed 404 -- the id is a
 * placeholder for a create the server has never heard of. `flushOutbox`'s multipart branch is
 * the last line of defense against that (IMPORTANT 3): it never forwards `body.activity_id`
 * verbatim without checking it first.
 */
describe('flushOutbox and a still-negative queued activity id', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('parks an orphaned upload dead instead of sending a guaranteed 404, when no live create will ever resolve its temp id', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, {
      id: 'orphan', kind: 'attachment.upload', path: '/objects/1/attachments',
      body: { activity_id: -5 }, blob: new Blob(['x']), filename: 'photo.jpg', attempts: 0,
    });
    globalThis.fetch = vi.fn(async () => { throw new Error('must never be called'); }) as unknown as typeof fetch;

    await flushOutbox();

    expect(globalThis.fetch).not.toHaveBeenCalled(); // never even attempted -- would only 404
    expect(await outboxDeadCount()).toBe(1);
  });

  /**
   * MINOR 2: this branch used to `throw new Error(...)`, an ordinary retryable failure --
   * which both burned an attempt on 'upload' (an op that did nothing wrong) AND stopped the
   * pass, so 'create', queued right behind it, was never even attempted. Three flushes parked
   * 'upload' dead while the create it was waiting on was still live and had never once been
   * sent. The fix (`SkipOp`, `./outbox.ts`) must leave 'upload' completely untouched and let
   * the pass carry on to 'create' in the very same pass.
   */
  it('leaves an upload waiting on its live parent create untouched, and still sends a later op in the same pass', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    // Enqueued out of the usual create-then-upload order on purpose: the safety net has to
    // consult the store's actual live ops, not assume its own dependent create already ran.
    await enqueue(store, {
      id: 'upload', kind: 'attachment.upload', path: '/objects/1/attachments',
      body: { activity_id: -7 }, blob: new Blob(['x']), filename: 'photo.jpg', attempts: 0,
    });
    await enqueue(store, { id: 'create', kind: 'activity.create', path: '/objects/1/activities', body: { title: 'x' }, tempId: -7, attempts: 0 });
    const urls: string[] = [];
    globalThis.fetch = vi.fn(async (url: string) => {
      urls.push(url);
      return jsonResponse(201, { id: 77 });
    }) as unknown as typeof fetch;

    await flushOutbox();

    // 'upload' is skipped, never attempted -- fetch is only ever called for 'create'.
    expect(urls).toEqual(['/api/objects/1/activities']);
    expect(await outboxDeadCount()).toBe(0);
    const upload = (await store.all()).find((o) => o.id === 'upload');
    // Left exactly alone: no attempt burned, not dead -- the guard must not kill the op it
    // exists to protect.
    expect(upload?.attempts).toBe(0);
    expect(upload?.dead).toBeFalsy();
    // 'create' having actually been sent this pass resolves the temp id into the still-queued
    // upload's body, even though the upload itself wasn't sent.
    expect(upload?.body.activity_id).toBe(77);
  });
});

/**
 * `onOutboxFlushed` listeners now receive this pass's temp-id -> real-id resolutions (see
 * `replay` in ../src/lib/outbox.ts), so a view that minted its own temp id (`ActivityForm.svelte`)
 * can learn its draft resolved without re-deriving it from the store.
 */
describe('onOutboxFlushed carries this pass\'s resolved ids', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('notifies listeners with the tempId -> realId map resolved during the pass', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'create', kind: 'activity.create', path: '/objects/1/activities', body: { title: 'x' }, tempId: -3, attempts: 0 });
    globalThis.fetch = vi.fn(async () => jsonResponse(201, { id: 55 })) as unknown as typeof fetch;

    const seen: Array<Map<number, number>> = [];
    const unsubscribe = onOutboxFlushed((resolved) => seen.push(resolved));
    await flushOutbox();
    unsubscribe();

    expect(seen).toHaveLength(1);
    expect(seen[0].get(-3)).toBe(55);
  });

  it('notifies with an empty map when the pass resolved nothing', async () => {
    const seen: Array<Map<number, number>> = [];
    const unsubscribe = onOutboxFlushed((resolved) => seen.push(resolved));
    await flushOutbox();
    unsubscribe();
    expect(seen).toEqual([new Map()]);
  });
});

/**
 * A kind `flushOutbox` has no send path for (today, anything but `activity.create` and
 * `attachment.upload`) must never be able to stall every op behind it -- see the removed
 * `throw new Error(...)` this replaces, and the head-of-line-blocking comment on `replay` in
 * `../src/lib/outbox.ts`.
 */
describe('flushOutbox and an unsupported op kind', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('parks an op it cannot send as dead and keeps going, instead of stopping the pass', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'unknown', kind: 'reminder.done', path: '/reminders/1/done', body: {}, attempts: 0 });
    await enqueue(store, { id: 'healthy', kind: 'activity.create', path: '/objects/1/activities', body: { title: 'x' }, attempts: 0 });

    const urls: string[] = [];
    globalThis.fetch = vi.fn(async (url: string) => {
      urls.push(url);
      return jsonResponse(201, { id: 1 });
    }) as unknown as typeof fetch;

    await flushOutbox();

    expect(urls).toEqual(['/api/objects/1/activities']); // the healthy op behind it still sent
    expect(await outboxDeadCount()).toBe(1);
  });
});

/**
 * IMPORTANT 1: `serialize` (`./outbox.ts`) only guards `flushOutbox` against ANOTHER call to
 * `flushOutbox` -- it does nothing to stop `ActivityForm.svelte`'s Save/Cancel from writing to
 * the very op a pass has already read and is mid-`send()` for. Both races were reproduced
 * directly against the pre-fix code: a pass's retry write-back (using the body/id it captured
 * at the TOP of the pass, before the UI write happened) landed AFTER the UI write and clobbered
 * it -- reverting an edited title back to the original on Save, and resurrecting an op Cancel
 * had already removed. `outboxLock` (`./outbox.ts`'s `createLock`, held by `doFlushOutbox` for
 * the whole pass and by `updateQueuedActivity`/`cancelQueuedActivity`) makes the two mutually
 * exclusive: a UI write can only run either fully before or fully after the pass, never interleaved.
 */
describe('IMPORTANT 1: a UI write must not interleave with an in-flight replay pass', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('a Save mid-send is not reverted by the pass\'s own failure write-back', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, {
      id: 'create', kind: 'activity.create', path: '/objects/1/activities',
      tempId: -1, body: { title: 'old' }, attempts: 0,
    });

    const calls: Array<Record<string, unknown>> = [];
    let rejectSend!: (e: unknown) => void;
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      calls.push(JSON.parse(init?.body as string));
      // The send this pass makes for 'create' never settles until the test says so -- this is
      // the window in which the user hits Save.
      return new Promise<Response>((_resolve, reject) => { rejectSend = reject; });
    }) as unknown as typeof fetch;

    const flushPromise = flushOutbox();
    await new Promise((r) => setTimeout(r, 0));
    expect(calls).toHaveLength(1);
    expect(calls[0].title).toBe('old'); // the pass is genuinely mid-send, with the OLD body

    // Save fires while that request is still in flight.
    const savePromise = updateQueuedActivity(-1, { title: 'new' });

    // The in-flight request then fails retryably (a dropped connection) -- pre-fix, the pass's
    // catch would write back `{ ...op, body: { title: 'old' }, attempts: 1 }` right on top of
    // whatever Save had just written, reverting the edit.
    rejectSend(new TypeError('Failed to fetch'));
    const [folded] = await Promise.all([savePromise, flushPromise]);

    expect(folded).toBe(true);
    const [row] = await store.all();
    expect(row.body.title).toBe('new'); // never reverted to 'old'
    expect(row.dead).toBeFalsy();
  });

  it('a Cancel mid-send is not resurrected by the pass\'s own failure write-back', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, {
      id: 'create', kind: 'activity.create', path: '/objects/1/activities',
      tempId: -1, body: { title: 'x' }, attempts: 0,
    });

    let rejectSend!: (e: unknown) => void;
    globalThis.fetch = vi.fn(async () => {
      return new Promise<Response>((_resolve, reject) => { rejectSend = reject; });
    }) as unknown as typeof fetch;

    const flushPromise = flushOutbox();
    await new Promise((r) => setTimeout(r, 0));
    expect(globalThis.fetch).toHaveBeenCalledTimes(1); // the pass is genuinely mid-send

    // Cancel fires while that request is still in flight.
    const cancelPromise = cancelQueuedActivity(-1);

    // The in-flight request then fails retryably -- pre-fix, the pass's catch would write the
    // op straight back (`dead: false`, `attempts: 1`), resurrecting exactly what Cancel had
    // just removed.
    rejectSend(new TypeError('Failed to fetch'));
    const [removed] = await Promise.all([cancelPromise, flushPromise]);

    expect(removed).toBe(true);
    expect(await store.all()).toHaveLength(0); // must not come back
  });
});

/**
 * MINOR 4(a): `retryDead` used to call `flushOutbox()` exactly once. If a pass was already
 * running when the dead ops were revived, that call just joined the ALREADY in-flight pass
 * (`serialize` shares one in-flight run) -- whose snapshot was taken before the revival and so
 * still shows those ops dead. The user's "try again" silently did nothing until some later,
 * unrelated trigger happened to flush again.
 */
describe('MINOR 4(a): retryDead must not silently join a pass that started before the revival', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('actually resends a revived op even when a pass was already in flight at the moment of revival', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'blocker', kind: 'activity.create', path: '/objects/1/activities', body: {}, attempts: 0 });
    await enqueue(store, { id: 'dead', kind: 'activity.create', path: '/objects/2/activities', body: {}, attempts: 3, dead: true });

    let releaseBlocker!: (v: Response) => void;
    const calls: string[] = [];
    globalThis.fetch = vi.fn(async (url: string) => {
      calls.push(url);
      if (url === '/api/objects/1/activities') {
        return new Promise<Response>((resolve) => { releaseBlocker = resolve; });
      }
      return jsonResponse(201, { id: 1 });
    }) as unknown as typeof fetch;

    // A pass starts and gets stuck mid-send on 'blocker' -- exactly the window in which the
    // user presses "try again".
    const stalePass = flushOutbox();
    await new Promise((r) => setTimeout(r, 0));
    expect(calls).toEqual(['/api/objects/1/activities']);

    const retryPromise = retryDead();

    // Let the stale pass's blocked send finally settle so both promises can resolve.
    releaseBlocker(jsonResponse(201, { id: 99 }));
    await Promise.all([stalePass, retryPromise]);

    // The revived op must actually have been resent, not silently left behind because
    // `retryDead` only joined the pass that predated its own revival.
    expect(calls).toContain('/api/objects/2/activities');
    expect((await store.all()).find((o) => o.id === 'dead')).toBeUndefined();
  });
});
