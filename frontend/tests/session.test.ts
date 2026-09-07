import { describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { enqueue, memoryStore, type OutboxStore } from '../src/lib/outbox';

/**
 * The session module is a small state machine now -- signed in, signed out, and "we cannot tell
 * yet", which are three different things -- and the outbox refuses to send until it says which.
 * Four review findings landed here (a swallowed status leaving the outbox disabled for the life
 * of the page, retries stacking without bound, Setup navigating to a dead end, and the
 * outbox-user wiring itself), and none was reachable from a test while the module could not be
 * imported outside a browser.
 */
function jsonResponse(status: number, body: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: (k: string) => (k.toLowerCase() === 'content-type' ? 'application/json' : null) },
    json: async () => body,
    text: async () => JSON.stringify(body),
  } as unknown as Response;
}

/** A server that answers each path from `routes`, recording what it was asked for. */
function serve(routes: Record<string, () => Response | Promise<Response>>) {
  const calls: string[] = [];
  globalThis.fetch = vi.fn(async (url: string) => {
    const path = url.replace('/api', '');
    calls.push(path.split('?')[0]);
    for (const [prefix, answer] of Object.entries(routes)) {
      if (path.startsWith(prefix)) return answer();
    }
    throw new TypeError('Failed to fetch');
  }) as unknown as typeof fetch;
  return calls;
}

const ME = { id: 7, username: 'ben', is_admin: true };
const signedIn = {
  '/auth/status': () => jsonResponse(200, { setup_required: false }),
  '/auth/me': () => jsonResponse(200, ME),
  '/settings': () => jsonResponse(200, { currency: 'EUR' }),
};

/**
 * A fresh module registry, so the session module starts over -- and so does the `api` module it
 * imports, which is why the outbox store has to be injected into THAT copy rather than into the
 * one this file imported statically.
 */
async function freshSession(store: OutboxStore = memoryStore()) {
  vi.resetModules();
  const api = await import('../src/lib/api');
  api.setOutboxStoreForTesting(store);
  const session = await import('../src/stores/session');
  session.setNavigateForTesting(() => {});
  return session;
}

describe('loadSession', () => {
  it('reports the session known and records who is signed in', async () => {
    serve(signedIn);
    const session = await freshSession();

    expect(await session.loadSession()).toBe(true);
    expect(get(session.user)).toEqual(ME);
    expect(get(session.currency)).toBe('EUR');
  });

  /**
   * The regression: `/api/auth/*` is NetworkOnly in the service worker, so an offline boot
   * rejects on the very first request. Recording that as "signed out" would be a lie the outbox
   * then acts on -- and leaving it recorded as ANY answer disabled sending for the life of the
   * page, so a queued write sat there with a valid cookie until a manual reload.
   */
  it('reports the session still unknown when it cannot be reached, and asserts nothing', async () => {
    serve({});
    const session = await freshSession();

    expect(await session.loadSession()).toBe(false);
    expect(get(session.user)).toBeUndefined(); // not `null`: undefined means "not loaded yet"
  });

  it('reports known-and-signed-out when the server says so', async () => {
    serve({
      '/auth/status': () => jsonResponse(200, { setup_required: false }),
      '/auth/me': () => jsonResponse(401, { code: 'unauthorized', message: 'log in' }),
    });
    const session = await freshSession();

    expect(await session.loadSession()).toBe(true);
    expect(get(session.user)).toBeNull();
  });

  it('reports still-unknown when the session request itself fails', async () => {
    serve({
      '/auth/status': () => jsonResponse(200, { setup_required: false }),
      '/auth/me': () => { throw new TypeError('Failed to fetch'); },
    });
    const session = await freshSession();

    expect(await session.loadSession()).toBe(false);
    expect(get(session.user)).toBeUndefined();
  });

  it('reports known when the instance has no users yet', async () => {
    serve({ '/auth/status': () => jsonResponse(200, { setup_required: true }) });
    const session = await freshSession();

    expect(await session.loadSession()).toBe(true);
    expect(get(session.setupRequired)).toBe(true);
    expect(get(session.user)).toBeNull();
  });

  /**
   * `sessionKnown` only flips after two awaited round trips, so behind a slow `/auth/status`
   * every tab switch used to start another complete attempt -- stacking auth round trips
   * without bound on exactly the flaky connection the retry exists for.
   */
  it('runs one attempt at a time however many callers ask', async () => {
    const calls = serve(signedIn);
    const session = await freshSession();

    const [a, b, c] = await Promise.all([session.loadSession(), session.loadSession(), session.loadSession()]);

    expect([a, b, c]).toEqual([true, true, true]);
    expect(calls.filter((p) => p === '/auth/status')).toHaveLength(1);
  });

  it('starts a genuinely new attempt once the previous one has finished', async () => {
    const calls = serve({});
    const session = await freshSession();

    expect(await session.loadSession()).toBe(false);
    expect(await session.loadSession()).toBe(false);

    expect(calls.filter((p) => p === '/auth/status')).toHaveLength(2);
  });

  /** Learning who is signed in is what lets the outbox send: it refuses while it cannot tell
   *  whose ops these are, so the queued write has to go out on the back of this. */
  it('sends what the outbox was holding once it knows the user', async () => {
    const store = memoryStore();
    await enqueue(store, { id: 'q', kind: 'activity.create', path: '/objects/1/activities', body: { title: 'Fuel' }, attempts: 0, userId: ME.id });

    const calls = serve({ ...signedIn, '/objects/1/activities': () => jsonResponse(201, { id: 1 }) });
    const session = await freshSession(store);

    await session.loadSession();
    // The flush is fired without being awaited, so let it settle.
    await new Promise((r) => setTimeout(r, 0));

    expect(calls).toContain('/objects/1/activities');
    expect(await store.all()).toEqual([]);
  });
});

describe('ending a session', () => {
  it('detaches the queue rather than clearing it, and stops it being sent', async () => {
    const store = memoryStore();
    const calls = serve({ ...signedIn, '/auth/logout': () => jsonResponse(204, null) });
    const session = await freshSession(store);
    await session.loadSession();

    await enqueue(store, { id: 'q', kind: 'activity.create', path: '/objects/1/activities', body: {}, attempts: 0, userId: ME.id });
    await session.logout();

    expect(get(session.user)).toBeNull();
    // Still there -- surviving a session is the point of the queue -- but nobody is signed in
    // to attribute a send to, so nothing goes out until its owner comes back.
    expect(await store.all()).toHaveLength(1);
    expect(calls).not.toContain('/objects/1/activities');
  });

  it('navigates to the login screen', async () => {
    serve({ ...signedIn, '/auth/logout': () => jsonResponse(204, null) });
    const session = await freshSession();
    const went: string[] = [];
    session.setNavigateForTesting((p) => went.push(p));

    await session.loadSession();
    await session.logout();

    expect(went).toEqual(['/login']);
  });
});

/**
 * A queued upload carries the photo itself, and the server has never seen it -- so of
 * everything the app stores locally, that is the one thing eviction destroys outright. Android
 * Chrome clears "best-effort" storage under space pressure, so the app asks for its origin to
 * be kept once there is a session worth keeping it for.
 */
describe('storage persistence', () => {
  it('asks the browser to keep the queue once the session is known', async () => {
    const persist = vi.fn(async () => true);
    Object.defineProperty(globalThis, 'navigator', {
      value: { storage: { persist, persisted: async () => false } },
      configurable: true,
    });
    serve(signedIn);
    const session = await freshSession();

    await session.loadSession();
    await new Promise((r) => setTimeout(r, 0)); // fire-and-forget

    expect(persist).toHaveBeenCalled();
  });

  it('does not ask again when the origin is already persistent', async () => {
    const persist = vi.fn(async () => true);
    Object.defineProperty(globalThis, 'navigator', {
      value: { storage: { persist, persisted: async () => true } },
      configurable: true,
    });
    serve(signedIn);
    const session = await freshSession();

    await session.loadSession();
    await new Promise((r) => setTimeout(r, 0));

    expect(persist).not.toHaveBeenCalled();
  });

  it('carries on where the browser has no such API', async () => {
    Object.defineProperty(globalThis, 'navigator', { value: {}, configurable: true });
    serve(signedIn);
    const session = await freshSession();

    // The point is that this resolves at all: an optimisation must not break signing in.
    expect(await session.loadSession()).toBe(true);
  });
});
