import { describe, it, expect, beforeEach, vi } from 'vitest';
import { cancelQueuedActivity, createObjectQueued, createQueued, deadOps, flushOutbox, onOutboxFlushed, outboxPending, pendingObjectOps, retryDead, setOutboxStoreForTesting, setOutboxUser, setUnauthorizedHandler, updateQueuedActivity, uploadQueued, ApiError, outboxDeadCount } from '../src/lib/api';
import { memoryStore, enqueue } from '../src/lib/outbox';

// Every flush in this file stands in for one made by a signed-in user: `flushOutbox` sends
// nothing at all when it does not know who is asking (see `doFlushOutbox` in ../src/lib/api.ts),
// which is what keeps the boot flush from replaying one user's queue under another's cookie.
// The few tests that are ABOUT not knowing set the user to null themselves.
beforeEach(() => setOutboxUser(1));

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

describe('offline object creation', () => {
  beforeEach(() => setOutboxStoreForTesting(memoryStore()));

  it('keeps the object visible and replays with one stable client_uuid', async () => {
    const bodies: Array<Record<string, unknown>> = [];
    let offline = true;
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      bodies.push(JSON.parse(init?.body as string));
      if (offline) throw new TypeError('offline');
      return jsonResponse(201, { id: 44 });
    }) as unknown as typeof fetch;

    expect(await createObjectQueued({ name: 'Oil tank', type: 'home' }, -9)).toBeNull();
    const queued = await pendingObjectOps();
    expect(queued).toHaveLength(1);
    expect(queued[0].tempId).toBe(-9);

    offline = false;
    await flushOutbox();
    expect(bodies[1].client_uuid).toBe(bodies[0].client_uuid);
    expect(await pendingObjectOps()).toHaveLength(0);
  });

  it('rewrites activities queued against a pending object to its real id', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'object-op', kind: 'object.create', path: '/objects', body: { name: 'Water meter', type: 'home' }, tempId: -9, attempts: 0, userId: 1 });
    await enqueue(store, { id: 'activity-op', kind: 'activity.create', path: '/objects/-9/activities', body: { date: '2026-09-20', category: 'usage' }, attempts: 0, userId: 1 });
    const urls: string[] = [];
    globalThis.fetch = vi.fn(async (url: string) => {
      urls.push(String(url));
      return jsonResponse(201, { id: urls.length === 1 ? 44 : 55 });
    }) as unknown as typeof fetch;

    await flushOutbox();

    expect(urls.some((url) => url.endsWith('/objects/44/activities'))).toBe(true);
    expect(await store.all()).toEqual([]);
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

/**
 * `main.ts` fires a flush at module load, before anything knows whether the session is still
 * valid, and `visibilitychange` fires one every time the tab comes back. An expired cookie --
 * or a password change, which now ends every other session -- therefore meets the queue with a
 * 401 on every op. A 401 is an `ApiError` in the 4xx range, so `isRejection` used to be true
 * for it and every queued write was parked permanently dead before the user had even seen the
 * login screen; logging back in replayed nothing, and the writes survived only as rows in
 * Settings' failed list for the user to notice and retry by hand.
 *
 * Being logged out is the one 4xx that is not about the request at all: it is fixed by logging
 * back in, after which the very same op succeeds.
 */
describe('flushOutbox against an expired session', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('leaves the queue intact on a 401 and replays it after the user logs back in', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'first', kind: 'activity.create', path: '/objects/1/activities', body: { title: 'a' }, attempts: 0 });
    await enqueue(store, { id: 'second', kind: 'activity.create', path: '/objects/1/activities', body: { title: 'b' }, attempts: 0 });

    let loggedIn = false;
    const sent: string[] = [];
    globalThis.fetch = vi.fn(async (url: string, init?: RequestInit) => {
      if (!loggedIn) return jsonResponse(401, { code: 'unauthorized', message: 'log in' });
      sent.push(JSON.parse(init?.body as string).title);
      return jsonResponse(201, { id: sent.length });
    }) as unknown as typeof fetch;

    await flushOutbox();

    expect(sent).toEqual([]);
    expect(await outboxDeadCount()).toBe(0);
    const parked = await store.all();
    expect(parked).toHaveLength(2);
    // No attempt burned either: a 401 says nothing about the op, so it must not count against
    // the retry budget that eventually parks an op dead for good.
    expect(parked.map((o) => o.attempts)).toEqual([0, 0]);

    loggedIn = true;
    await flushOutbox();

    expect(sent).toEqual(['a', 'b']);
    expect(await store.all()).toHaveLength(0);
  });

  it('still parks a genuine 4xx rejection dead', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'bad', kind: 'activity.create', path: '/objects/1/activities', body: { title: '' }, attempts: 0 });

    globalThis.fetch = vi.fn(async () => jsonResponse(400, { code: 'bad_request', message: 'title required' })) as unknown as typeof fetch;

    await flushOutbox();

    expect(await outboxDeadCount()).toBe(1);
  });
});

/**
 * The other half of "an expired session must not cost the user their writes": `replay` keeps
 * what is already queued (above), and these keep what has not been queued yet. A session that
 * lapses while a form is open used to throw the 401 straight back at the caller -- the
 * unauthorized handler navigates to /login, and whatever was typed existed nowhere else.
 */
describe('queuing a write that meets an expired session', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('queues a create on a 401 and sends it once the user is back, with the same op id', async () => {
    let loggedIn = false;
    const bodies: Array<Record<string, unknown>> = [];
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      const body = JSON.parse(init?.body as string);
      bodies.push(body);
      if (!loggedIn) return jsonResponse(401, { code: 'unauthorized', message: 'log in' });
      return jsonResponse(201, { id: 7 });
    }) as unknown as typeof fetch;

    const result = await createQueued('/objects/1/activities', { title: 'Fuel' });
    expect(result).toBeNull(); // queued rather than lost
    expect(await outboxDeadCount()).toBe(0);

    loggedIn = true;
    await flushOutbox();

    expect(bodies).toHaveLength(2);
    expect(bodies[1].title).toBe('Fuel');
    // The idempotency guarantee still holds across the login: same op id, so if the server had
    // in fact applied the first attempt, the replay resolves to that row instead of a second one.
    expect(bodies[1].client_op_id).toBe(bodies[0].client_op_id);
  });

  it('queues an upload on a 401 too', async () => {
    globalThis.fetch = vi.fn(async () => jsonResponse(401, { code: 'unauthorized', message: 'log in' })) as unknown as typeof fetch;

    const result = await uploadQueued('/objects/1/attachments', new Blob(['x']), 'photo.png', 5);
    expect(result).toBeNull();
    expect(await outboxDeadCount()).toBe(0);
  });

  it('still throws a genuine rejection back at the caller rather than queuing it', async () => {
    globalThis.fetch = vi.fn(async () => jsonResponse(400, { code: 'bad_request', message: 'title required' })) as unknown as typeof fetch;

    await expect(createQueued('/objects/1/activities', { title: '' })).rejects.toThrow();
  });
});

/**
 * The outbox is one IndexedDB per origin, shared by every account that signs in on the device,
 * and a queued write deliberately outlives the session that made it. Without an owner on each
 * op, the next person to log in replays someone else's writes under their own session: the
 * server refuses them on ownership, `replay` parks them permanently dead, and the first user's
 * entries -- titles and all -- turn up in the second user's pending and failed lists.
 */
describe('an outbox shared by two users of one device', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
    setOutboxUser(null);
  });

  it('never sends, counts or shows a write queued by somebody else', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'theirs', kind: 'activity.create', path: '/objects/9/activities', body: { title: 'Their oil change' }, attempts: 0, userId: 1 });
    await enqueue(store, { id: 'ours', kind: 'activity.create', path: '/objects/3/activities', body: { title: 'Our fuel' }, attempts: 0, userId: 2 });

    setOutboxUser(2);
    const sent: string[] = [];
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      sent.push(JSON.parse(init?.body as string).title);
      return jsonResponse(201, { id: 1 });
    }) as unknown as typeof fetch;

    await flushOutbox();

    expect(sent).toEqual(['Our fuel']);
    // Left exactly as it was, for its own user to send when they sign back in.
    const theirs = (await store.all()).find((o) => o.id === 'theirs');
    expect(theirs?.dead).toBeFalsy();
    expect(theirs?.attempts).toBe(0);

    expect(await outboxPending()).toBe(0); // ours sent, theirs is not ours to count
    expect(await outboxDeadCount()).toBe(0);
    expect((await deadOps()).map((o) => o.id)).toEqual([]);
  });

  it('sends it once its own user signs back in', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'theirs', kind: 'activity.create', path: '/objects/9/activities', body: { title: 'Their oil change' }, attempts: 0, userId: 1 });

    const sent: string[] = [];
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      sent.push(JSON.parse(init?.body as string).title);
      return jsonResponse(201, { id: 1 });
    }) as unknown as typeof fetch;

    setOutboxUser(2);
    await flushOutbox();
    expect(sent).toEqual([]);

    setOutboxUser(1);
    await flushOutbox();
    expect(sent).toEqual(['Their oil change']);
    expect(await store.all()).toHaveLength(0);
  });

  it('tags what it queues with the user who queued it', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    setOutboxUser(7);
    globalThis.fetch = vi.fn(async () => { throw new TypeError('Failed to fetch'); }) as unknown as typeof fetch;

    await createQueued('/objects/1/activities', { title: 'x' });

    expect((await store.all())[0].userId).toBe(7);
  });

  it('treats a record queued before owners existed as the current user\'s', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'legacy', kind: 'activity.create', path: '/objects/1/activities', body: { title: 'Old' }, attempts: 0 });

    setOutboxUser(4);
    const sent: string[] = [];
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      sent.push(JSON.parse(init?.body as string).title);
      return jsonResponse(201, { id: 1 });
    }) as unknown as typeof fetch;

    await flushOutbox();

    expect(sent).toEqual(['Old']);
  });
});

/**
 * The 401 path is exactly where the owner is easiest to lose: `api()` runs the unauthorized
 * handler on its way out, and that handler clears the signed-in user. Reading the owner in
 * `createQueued`'s catch therefore attributed the write to nobody -- leaving it untagged in a
 * queue shared with every other account on the device, for the next person to claim.
 */
describe('the owner recorded on a write queued by an expired session', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
    setOutboxUser(null);
    setUnauthorizedHandler(() => {});
  });

  it('is whoever was signed in when the write was made, not nobody', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    setOutboxUser(3);
    // What ../stores/session.ts installs: a 401 ends the session, outbox owner included.
    setUnauthorizedHandler(() => setOutboxUser(null));
    globalThis.fetch = vi.fn(async () => jsonResponse(401, { code: 'unauthorized', message: 'log in' })) as unknown as typeof fetch;

    await createQueued('/objects/1/activities', { title: 'Hydraulic oil' });

    expect((await store.all()).map((o) => o.userId)).toEqual([3]);
  });

  it('is recorded the same way for a queued upload', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    setOutboxUser(3);
    setUnauthorizedHandler(() => setOutboxUser(null));
    globalThis.fetch = vi.fn(async () => jsonResponse(401, { code: 'unauthorized', message: 'log in' })) as unknown as typeof fetch;

    await uploadQueued('/objects/1/attachments', new Blob(['x']), 'photo.png');

    expect((await store.all()).map((o) => o.userId)).toEqual([3]);
  });
});

/**
 * `main.ts` flushes at module load, before `App.svelte`'s onMount has called `loadSession`,
 * which itself needs two round trips before it knows who is signed in. That boot flush used to
 * run unattributed and, since an unknown user counted as "ours", replayed whatever was queued
 * under whatever cookie happened to still be valid -- on a shared device, one user's writes
 * under another's session, refused on ownership and parked dead.
 */
describe('flushing before the session is known', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
    setOutboxUser(null);
  });

  it('sends nothing at all, and sends normally once the user is known', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'a', kind: 'activity.create', path: '/objects/1/activities', body: { title: 'Theirs' }, attempts: 0, userId: 1 });

    const sent: string[] = [];
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      sent.push(JSON.parse(init?.body as string).title);
      return jsonResponse(201, { id: 1 });
    }) as unknown as typeof fetch;

    await flushOutbox(); // the boot flush: nobody is signed in yet
    expect(sent).toEqual([]);
    expect((await store.all())[0].dead).toBeFalsy();

    setOutboxUser(1);
    await flushOutbox();
    expect(sent).toEqual(['Theirs']);
  });
});

/** Reviving a parked op is the owner's decision, not whoever happens to be signed in. */
describe('retryDead on a device with two users', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
    setOutboxUser(null);
  });

  it('revives only the signed-in user\'s own dead ops', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'theirs', kind: 'activity.create', path: '/objects/9/activities', body: {}, attempts: 3, dead: true, userId: 1 });
    await enqueue(store, { id: 'ours', kind: 'activity.create', path: '/objects/3/activities', body: { title: 'Ours' }, attempts: 3, dead: true, userId: 2 });

    setOutboxUser(2);
    globalThis.fetch = vi.fn(async () => jsonResponse(201, { id: 1 })) as unknown as typeof fetch;

    await retryDead();

    // Ours was revived and sent; theirs was never touched, so it is still parked for its owner
    // to decide about rather than being retried and re-parked behind their back.
    expect((await store.all()).map((o) => [o.id, o.dead])).toEqual([['theirs', true]]);
  });
});

/**
 * Views reload off this rather than diffing the queue themselves. A view's own snapshot is only
 * ever as fresh as its last load, so it misses anything queued while it sat there -- a photo
 * attached from another tab of the same page, say -- and then skips the reload when that write
 * finally lands. Whether the PASS changed anything is not subject to that.
 */
describe('what a flush pass reports to its listeners', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('reports no change for a pass over an empty queue', async () => {
    const seen: boolean[] = [];
    const off = onOutboxFlushed((_resolved, changed) => seen.push(changed));
    globalThis.fetch = vi.fn(async () => jsonResponse(201, { id: 1 })) as unknown as typeof fetch;

    await flushOutbox();

    off();
    expect(seen).toEqual([false]);
  });

  it('reports a change when an op is sent, and when one is parked dead', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'ok', kind: 'activity.create', path: '/objects/1/activities', body: {}, attempts: 0 });
    await enqueue(store, { id: 'bad', kind: 'reminder.done', path: '/reminders/1/done', body: {}, attempts: 0 });

    const seen: boolean[] = [];
    const off = onOutboxFlushed((_resolved, changed) => seen.push(changed));
    globalThis.fetch = vi.fn(async () => jsonResponse(201, { id: 1 })) as unknown as typeof fetch;

    await flushOutbox(); // sends 'ok', parks 'bad' (no send path for its kind)
    await flushOutbox(); // nothing left that can change
    off();

    expect(seen).toEqual([true, false]);
  });

  it('still reports to listeners on a pass that is skipped for want of a user', async () => {
    setOutboxUser(null);
    const seen: boolean[] = [];
    const off = onOutboxFlushed((_resolved, changed) => seen.push(changed));
    globalThis.fetch = vi.fn(async () => jsonResponse(201, { id: 1 })) as unknown as typeof fetch;

    await flushOutbox();

    off();
    expect(seen).toEqual([false]);
  });
});

/** With nobody signed in there is no "our own" to filter to -- `isOurs` widens to everything. */
describe('retryDead with no signed-in user', () => {
  it('revives nothing at all', async () => {
    const store = memoryStore();
    setOutboxStoreForTesting(store);
    await enqueue(store, { id: 'theirs', kind: 'activity.create', path: '/objects/9/activities', body: {}, attempts: 3, dead: true, userId: 1 });
    setOutboxUser(null);
    globalThis.fetch = vi.fn(async () => jsonResponse(201, { id: 1 })) as unknown as typeof fetch;

    await retryDead();

    expect((await store.all())[0].dead).toBe(true);
  });
});

/**
 * A pass that cannot say what it did must not claim it did nothing: a view told "no change"
 * skips its reload, so a write that really did reach the server is left rendered as a dimmed
 * pending row that never becomes real.
 */
describe('what a flush pass reports when it does not finish cleanly', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('reports a change when the op was sent but could not be removed', async () => {
    const inner = memoryStore();
    await enqueue(inner, { id: 'x', kind: 'activity.create', path: '/objects/1/activities', body: {}, attempts: 0 });
    // The send succeeds and the server holds the write; only the local cleanup fails.
    setOutboxStoreForTesting({ ...inner, remove: async () => { throw new Error('IndexedDB went away'); } });

    const seen: boolean[] = [];
    const off = onOutboxFlushed((_r, changed) => seen.push(changed));
    globalThis.fetch = vi.fn(async () => jsonResponse(201, { id: 1 })) as unknown as typeof fetch;

    await flushOutbox();
    off();

    expect(seen).toEqual([true]);
  });

  /**
   * The queue looks untouched afterwards -- the send failed and the write-back that would have
   * recorded the attempt failed too -- so a snapshot comparison alone says "no change". Only
   * the fact that the pass did not finish tells the truth: it cannot account for what it did.
   */
  it('reports a change when the pass throws and leaves the queue looking untouched', async () => {
    const inner = memoryStore();
    await enqueue(inner, { id: 'x', kind: 'activity.create', path: '/objects/1/activities', body: {}, attempts: 0 });
    setOutboxStoreForTesting({ ...inner, put: async () => { throw new Error('IndexedDB went away'); } });

    const seen: boolean[] = [];
    const off = onOutboxFlushed((_r, changed) => seen.push(changed));
    // A dropped connection, so `replay` takes the retry path -- whose write-back is what throws.
    globalThis.fetch = vi.fn(async () => { throw new TypeError('Failed to fetch'); }) as unknown as typeof fetch;

    await flushOutbox().catch(() => {});
    off();

    expect(seen).toEqual([true]);
  });
});
