import { afterEach, describe, expect, it, vi } from 'vitest';
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

/** An in-memory `localStorage`: vitest runs in node, which has none. */
function installStorage(): Storage {
  const m = new Map<string, string>();
  const s = {
    get length() { return m.size; },
    clear: () => m.clear(),
    getItem: (k: string) => m.get(k) ?? null,
    key: (i: number) => [...m.keys()][i] ?? null,
    removeItem: (k: string) => { m.delete(k); },
    setItem: (k: string, v: string) => { m.set(k, String(v)); },
  } as Storage;
  Object.defineProperty(globalThis, 'localStorage', { value: s, configurable: true });
  return s;
}

/** Records which service-worker caches were deleted, and in what order relative to other events. */
function installCaches(log: string[]) {
  globalThis.caches = { delete: vi.fn(async (name: string) => { log.push(`delete:${name}`); return true; }) } as unknown as CacheStorage;
}

/**
 * Like `installCaches`, but each delete only finishes a macrotask later, logging `deleted:` when
 * it does. A real `caches.delete` is asynchronous, and until it resolves a fetch can still be
 * answered from the old cache -- so "called before the user is set" proves nothing; "finished
 * before the user is set" is the ordering that matters.
 */
function installSlowCaches(log: string[]) {
  globalThis.caches = {
    delete: vi.fn((name: string) => new Promise<boolean>((resolve) => {
      setTimeout(() => { log.push(`deleted:${name}`); resolve(true); }, 0);
    })),
  } as unknown as CacheStorage;
}

const PROFILE = { id: 7, username: 'ben', is_admin: true, lang: 'de' };

/**
 * The caches outlive the session (they are on disk), so they must follow whoever is signed in:
 * kept for the same person, cleared before anything loads for a different one. And the last
 * person's profile lets the app open offline -- without ever counting as a known session.
 */
describe('offline start and cache ownership', () => {
  afterEach(() => {
    // @ts-expect-error -- test-only cleanup of globals this suite installs
    delete globalThis.caches;
    // @ts-expect-error -- as above
    delete globalThis.localStorage;
  });

  it('opens as the remembered user when the server cannot be reached, without knowing the session', async () => {
    const storage = installStorage();
    storage.setItem('logb.session.profile', JSON.stringify(PROFILE));
    storage.setItem('logb.cache.user', '7');
    const store = memoryStore();
    await enqueue(store, { id: 'q', kind: 'activity.create', path: '/objects/1/activities', body: {}, attempts: 0, userId: 7 });
    const calls = serve({});
    const session = await freshSession(store);
    const api = await import('../src/lib/api');

    expect(await session.loadSession()).toBe(false); // still unknown: the retry keeps trying
    expect(get(session.user)).toMatchObject(PROFILE);
    expect(get(session.offline)).toBe(true);

    // Queued, not sent: nothing has confirmed the cookie still belongs to this person.
    await api.flushOutbox();
    expect(calls).not.toContain('/objects/1/activities');
    expect(await store.all()).toHaveLength(1);
  });

  it('stays unknown, with no user, when nothing is remembered', async () => {
    installStorage();
    serve({});
    const session = await freshSession();

    expect(await session.loadSession()).toBe(false);
    expect(get(session.user)).toBeUndefined();
    expect(get(session.offline)).toBe(false);
  });

  it('keeps the caches when the real check later confirms the same user, and then sends', async () => {
    const storage = installStorage();
    storage.setItem('logb.session.profile', JSON.stringify(PROFILE));
    storage.setItem('logb.cache.user', '7');
    const log: string[] = [];
    installCaches(log);
    const store = memoryStore();
    await enqueue(store, { id: 'q', kind: 'activity.create', path: '/objects/1/activities', body: {}, attempts: 0, userId: 7 });
    serve({});
    const session = await freshSession(store);
    expect(await session.loadSession()).toBe(false);

    const calls = serve({ ...signedIn, '/auth/me': () => jsonResponse(200, { ...PROFILE }), '/objects/1/activities': () => jsonResponse(201, { id: 1 }) });
    expect(await session.loadSession()).toBe(true);
    await new Promise((r) => setTimeout(r, 0));

    expect(get(session.offline)).toBe(false);
    expect(log).toEqual([]);
    expect(calls).toContain('/objects/1/activities');
    expect(await store.all()).toEqual([]);
  });

  it('clears the caches before anything loads when a different user turns out to be signed in', async () => {
    const storage = installStorage();
    storage.setItem('logb.session.profile', JSON.stringify(PROFILE));
    storage.setItem('logb.cache.user', '7');
    const log: string[] = [];
    installSlowCaches(log);
    serve({});
    const session = await freshSession();
    expect(await session.loadSession()).toBe(false);

    const OTHER = { id: 8, username: 'anna', is_admin: false, lang: 'en' };
    serve({
      '/auth/status': () => jsonResponse(200, { setup_required: false }),
      '/auth/me': () => jsonResponse(200, OTHER),
      '/settings': () => { log.push('fetch:/settings'); return jsonResponse(200, { currency: 'EUR' }); },
    });
    session.user.subscribe((u) => { if (u && u.id === 8) log.push('user:8'); });
    expect(await session.loadSession()).toBe(true);

    // The deletes have FINISHED (not merely started) before the new user is set.
    expect(log.slice(0, 3)).toEqual(['deleted:logb-api', 'deleted:logb-files', 'user:8']);
    expect(log.indexOf('fetch:/settings')).toBeGreaterThan(log.indexOf('user:8'));
    expect(storage.getItem('logb.cache.user')).toBe('8');
    expect(JSON.parse(storage.getItem('logb.session.profile')!)).toEqual(OTHER);
    expect(get(session.offline)).toBe(false);
  });

  it('an offline start with caches owned by someone else finishes clearing them before showing the profile', async () => {
    const storage = installStorage();
    storage.setItem('logb.session.profile', JSON.stringify(PROFILE));
    storage.setItem('logb.cache.user', '9'); // half-written or edited: not provably PROFILE's
    const log: string[] = [];
    installSlowCaches(log);
    serve({});
    const session = await freshSession();
    session.user.subscribe((u) => { if (u && u.id === 7) log.push('user:7'); });

    expect(await session.loadSession()).toBe(false);

    expect(log).toEqual(['deleted:logb-api', 'deleted:logb-files', 'user:7']);
    expect(get(session.offline)).toBe(true);
  });

  /** A reset database starts user ids over, so a new user 7 must not inherit the old one's caches. */
  it('forgets the profile and the cache owner when the instance needs setup', async () => {
    const storage = installStorage();
    storage.setItem('logb.session.profile', JSON.stringify(PROFILE));
    storage.setItem('logb.cache.user', '7');
    serve({ '/auth/status': () => jsonResponse(200, { setup_required: true }) });
    const session = await freshSession();

    expect(await session.loadSession()).toBe(true);

    expect(storage.getItem('logb.session.profile')).toBeNull();
    expect(storage.getItem('logb.cache.user')).toBeNull();
  });

  it('clears on a first sign-in with no owner recorded, and remembers who signed in', async () => {
    const storage = installStorage();
    const log: string[] = [];
    installCaches(log);
    serve({ ...signedIn, '/auth/login': () => jsonResponse(200, { ...PROFILE }) });
    const session = await freshSession();

    await session.login('ben', 'pw');

    expect(log).toEqual(['delete:logb-api', 'delete:logb-files']);
    expect(storage.getItem('logb.cache.user')).toBe('7');
    expect(JSON.parse(storage.getItem('logb.session.profile')!)).toEqual(PROFILE);
  });

  it('a 401 from the real check ends the offline session and forgets the profile', async () => {
    const storage = installStorage();
    storage.setItem('logb.session.profile', JSON.stringify(PROFILE));
    storage.setItem('logb.cache.user', '7');
    serve({});
    const session = await freshSession();
    expect(await session.loadSession()).toBe(false);

    serve({
      '/auth/status': () => jsonResponse(200, { setup_required: false }),
      '/auth/me': () => jsonResponse(401, { code: 'unauthorized', message: 'log in' }),
    });
    expect(await session.loadSession()).toBe(true);

    expect(get(session.user)).toBeNull();
    expect(get(session.offline)).toBe(false);
    expect(storage.getItem('logb.session.profile')).toBeNull();
    expect(storage.getItem('logb.cache.user')).toBeNull();
  });

  it('logout and logout everywhere forget the profile and the cache owner', async () => {
    for (const end of ['logout', 'logoutEverywhere'] as const) {
      const storage = installStorage();
      serve({ ...signedIn, '/auth/logout': () => jsonResponse(204, null), '/auth/logout-all': () => jsonResponse(204, null) });
      const session = await freshSession();
      await session.loadSession();
      expect(storage.getItem('logb.session.profile')).not.toBeNull();
      expect(storage.getItem('logb.cache.user')).toBe('7');

      await session[end]();

      expect(storage.getItem('logb.session.profile')).toBeNull();
      expect(storage.getItem('logb.cache.user')).toBeNull();
    }
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
