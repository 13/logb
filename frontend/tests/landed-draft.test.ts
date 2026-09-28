import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cancelQueuedActivity, cancelQueuedObject, createObjectQueued, createQueued, flushOutbox, pendingActivityOps, pendingObjectOps, setOutboxStoreForTesting, updateQueuedActivity, updateQueuedObject } from '../src/lib/api-outbox';
import { setOutboxUser } from '../src/lib/api';
import { enqueue, memoryStore, type OutboxStore } from '../src/lib/outbox';

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
 * Just enough of the server for a create whose answer never arrives: it keeps the row, answers a
 * repeat of the same idempotency key with that row and 200 (a fresh one gets 201), and applies
 * PATCH and DELETE to it. `loseNextAnswer` makes the next request land and its answer vanish --
 * the save that timed out after the server had already done the work.
 */
function fakeServer() {
  const rows = new Map<number, Record<string, unknown>>();
  const byKey = new Map<string, number>();
  const requests: Array<{ method: string; url: string; body?: Record<string, unknown> }> = [];
  let nextId = 70;
  let lose = false;
  let drop = false;
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    const method = init?.method ?? 'GET';
    const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined;
    requests.push({ method, url, body });
    if (drop) { drop = false; throw new TypeError('Failed to fetch'); }
    let res: Response;
    const one = url.match(/^\/api\/(?:activities|objects)\/(\d+)$/);
    if (method === 'POST') {
      const key = (body.client_op_id ?? body.client_uuid) as string;
      const known = byKey.get(key);
      if (known !== undefined) {
        res = rows.has(known) ? jsonResponse(200, rows.get(known)) : jsonResponse(409, { error: 'conflict' });
      } else {
        const id = nextId++;
        rows.set(id, { ...body, id, created_at: '2026-09-28T10:00:00Z' });
        byKey.set(key, id);
        res = jsonResponse(201, rows.get(id));
      }
    } else if (method === 'PATCH' && one && rows.has(Number(one[1]))) {
      rows.set(Number(one[1]), { ...rows.get(Number(one[1])), ...body });
      res = jsonResponse(200, rows.get(Number(one[1])));
    } else if (method === 'DELETE' && one && rows.has(Number(one[1]))) {
      rows.delete(Number(one[1]));
      res = { ok: true, status: 204, headers: { get: () => null } } as unknown as Response;
    } else {
      res = jsonResponse(404, { error: 'not_found' });
    }
    if (lose) { lose = false; throw new TypeError('Failed to fetch'); }
    return res;
  });
  return { rows, requests, fetch, loseNextAnswer: () => { lose = true; }, dropNextRequest: () => { drop = true; } };
}

/**
 * A create that timed out may have landed anyway. Folding a later edit into its queued op, or
 * cancelling it, used to act on the queue alone: the replay's repeat of the idempotency key was
 * answered with the FIRST row, untouched, so the edit was silently lost -- and a cancelled draft
 * stayed on the server for good.
 */
describe('a draft whose create may have landed before it was queued', () => {
  let store: OutboxStore;
  let server: ReturnType<typeof fakeServer>;

  beforeEach(() => {
    setOutboxUser(1);
    store = memoryStore();
    setOutboxStoreForTesting(store);
    server = fakeServer();
    globalThis.fetch = server.fetch as unknown as typeof fetch;
  });

  afterEach(() => vi.unstubAllGlobals());

  const draft = { date: '2026-09-28', category: 'repair', title: 'Repair', notes: '' };

  it('gets an edit folded in afterwards onto the row that landed', async () => {
    server.loseNextAnswer();
    expect(await createQueued('/objects/1/activities', draft, -101)).toBeNull();
    expect([...server.rows.values()][0].title).toBe('Repair'); // it landed after all

    expect(await updateQueuedActivity(-101, { ...draft, title: 'Chain' })).toBe(true);
    await flushOutbox();

    const [row] = [...server.rows.values()];
    expect(server.rows.size).toBe(1);
    expect(row.title).toBe('Chain');
    const patch = server.requests.find((r) => r.method === 'PATCH')!;
    expect(patch.url).toBe(`/api/activities/${row.id}`);
    // A full body, as an activity PATCH needs, stamped so the server does not take it for stale.
    expect(patch.body).toMatchObject({ date: '2026-09-28', category: 'repair', title: 'Chain' });
    expect(typeof patch.body!.edited_at).toBe('string');
    expect(await store.all()).toHaveLength(0);
  });

  it('sends no extra request when the replay itself made the row', async () => {
    server.dropNextRequest();
    expect(await createQueued('/objects/1/activities', draft, -102)).toBeNull();
    expect(server.rows.size).toBe(0); // never arrived

    await updateQueuedActivity(-102, { ...draft, title: 'Chain' });
    await flushOutbox();

    expect([...server.rows.values()][0].title).toBe('Chain');
    expect(server.requests.map((r) => r.method)).toEqual(['POST', 'POST']);
  });

  it('deletes a cancelled draft that landed, and keeps it off the pending list meanwhile', async () => {
    server.loseNextAnswer();
    await createQueued('/objects/1/activities', draft, -103);
    expect(server.rows.size).toBe(1);

    expect(await cancelQueuedActivity(-103)).toBe(true);
    expect(await pendingActivityOps(1, [])).toEqual([]);

    await flushOutbox();
    expect(server.rows.size).toBe(0);
    expect(server.requests.at(-1)).toMatchObject({ method: 'DELETE' });
    expect(await store.all()).toHaveLength(0);
  });

  it('drops a cancelled draft outright when the device was offline and nothing left it', async () => {
    vi.stubGlobal('navigator', { onLine: false });
    server.dropNextRequest();
    await createQueued('/objects/1/activities', draft, -104);
    vi.stubGlobal('navigator', { onLine: true });

    expect(await cancelQueuedActivity(-104)).toBe(true);
    expect(await store.all()).toHaveLength(0);
    await flushOutbox();
    expect(server.requests).toHaveLength(1); // no create-then-delete for a row that never was
  });

  it('counts a replay that got no answer as possibly landed too', async () => {
    vi.stubGlobal('navigator', { onLine: false });
    server.dropNextRequest();
    await createQueued('/objects/1/activities', draft, -105);
    vi.stubGlobal('navigator', { onLine: true });

    server.loseNextAnswer();
    await flushOutbox(); // lands, answer lost
    expect(server.rows.size).toBe(1);

    await cancelQueuedActivity(-105);
    await flushOutbox();
    expect(server.rows.size).toBe(0);
  });

  it('lets a cancelled draft go quietly when the server refuses its create', async () => {
    await enqueue(store, { id: 'gone', kind: 'activity.create', path: '/objects/1/activities', tempId: -106, body: draft, attempts: 1, maybeLanded: true });
    server.fetch.mockImplementationOnce(async () => jsonResponse(409, { error: 'op_id_conflict' }));

    await cancelQueuedActivity(-106);
    await flushOutbox();
    expect(await store.all()).toHaveLength(0); // not parked dead: there is nothing to undo
  });

  it('does the same for an object: the folded edit is sent, and a cancelled one is deleted', async () => {
    server.loseNextAnswer();
    expect(await createObjectQueued({ name: 'Bike', type: 'bicycle' }, -107)).toBeNull();
    expect(await updateQueuedObject(-107, { name: 'Road bike', type: 'bicycle' })).toBe(true);
    await flushOutbox();
    expect([...server.rows.values()][0].name).toBe('Road bike');

    server.loseNextAnswer();
    await createObjectQueued({ name: 'Car', type: 'car' }, -108);
    expect(server.rows.size).toBe(2);
    expect(await cancelQueuedObject(-108)).toBe(true);
    expect((await pendingObjectOps()).map((o) => o.tempId)).toEqual([]);
    await flushOutbox();
    expect([...server.rows.values()].map((r) => r.name)).toEqual(['Road bike']);
  });
});
