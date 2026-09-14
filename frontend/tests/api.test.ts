import { describe, it, expect, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import { api, ApiError, clearServingSaved, isRejection, resetClockSkewForTesting, servedFromCache, servingSaved, setUnauthorizedHandler, fileUrl } from '../src/lib/api';

/** `dateHeader` defaults to absent, matching every existing call site of this helper: none of
 *  them cared about `servingSaved` before this response header existed. */
function mockFetch(status: number, body: unknown, dateHeader: string | null = null) {
  const res = {
    ok: status >= 200 && status < 300,
    status,
    headers: {
      get: (k: string) => {
        const key = k.toLowerCase();
        if (key === 'content-type') return body !== undefined ? 'application/json' : null;
        if (key === 'date') return dateHeader;
        return null;
      },
    },
    json: async () => body,
    text: async () => JSON.stringify(body),
  };
  globalThis.fetch = vi.fn(async () => res as unknown as Response);
}

describe('api', () => {
  beforeEach(() => setUnauthorizedHandler(() => {}));

  it('sends JSON and parses the reply', async () => {
    mockFetch(200, { id: 1 });
    const r = await api<{ id: number }>('POST', '/objects', { name: 'x' });
    expect(r.id).toBe(1);
    const [url, init] = (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/objects');
    expect(init.method).toBe('POST');
    expect((init.headers as Record<string, string>)['content-type']).toBe('application/json');
    expect(init.body).toBe('{"name":"x"}');
  });

  it('returns undefined for 204', async () => {
    mockFetch(204, undefined);
    expect(await api('DELETE', '/objects/1')).toBeUndefined();
  });

  it('throws ApiError with server code and message', async () => {
    mockFetch(400, { error: 'bad_request', message: 'name is required' });
    await expect(api('POST', '/objects', {})).rejects.toMatchObject({ status: 400, code: 'bad_request', message: 'name is required' });
    await expect(api('POST', '/objects', {})).rejects.toBeInstanceOf(ApiError);
  });

  it('calls the unauthorized handler on 401 outside /auth', async () => {
    const handler = vi.fn();
    setUnauthorizedHandler(handler);
    mockFetch(401, { error: 'unauthorized', message: 'authentication required' });
    await expect(api('GET', '/objects')).rejects.toBeInstanceOf(ApiError);
    expect(handler).toHaveBeenCalledTimes(1);
    await expect(api('POST', '/auth/login', {})).rejects.toBeInstanceOf(ApiError);
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('builds file urls', () => {
    expect(fileUrl(5)).toBe('/api/files/5');
    expect(fileUrl(5, true)).toBe('/api/files/5/thumb');
  });
});

// The whole offline/replay feature's safety rests on this classification: it decides whether a
// failed write is queued for later (safe, because client_op_id makes a replay idempotent) or
// reported straight back to the user as refused. Getting it backwards in either direction is a
// real bug (CRITICAL 2), not a style choice, so every branch gets its own case.
describe('isRejection', () => {
  it('is true for a 4xx ApiError -- the server received the request and refused it', () => {
    expect(isRejection(new ApiError(400, 'bad_request', 'nope'))).toBe(true);
    expect(isRejection(new ApiError(401, 'unauthorized', 'nope'))).toBe(true);
    expect(isRejection(new ApiError(403, 'forbidden', 'nope'))).toBe(true);
    expect(isRejection(new ApiError(404, 'not_found', 'nope'))).toBe(true);
    expect(isRejection(new ApiError(422, 'invalid', 'nope'))).toBe(true);
  });

  it('is false for a 5xx ApiError -- the server may have applied the write before failing', () => {
    expect(isRejection(new ApiError(500, 'error', 'boom'))).toBe(false);
    expect(isRejection(new ApiError(503, 'error', 'boom'))).toBe(false);
  });

  it('is false for a network failure, an aborted request, and a malformed response', () => {
    expect(isRejection(new TypeError('Failed to fetch'))).toBe(false);
    expect(isRejection(new DOMException('The user aborted a request.', 'AbortError'))).toBe(false);
    // handle() throws this when the body claims to be JSON but is not.
    expect(isRejection(new SyntaxError('Unexpected token < in JSON at position 0'))).toBe(false);
  });

  it('is false for anything that is not an ApiError at all', () => {
    expect(isRejection(new Error('anything else'))).toBe(false);
    expect(isRejection('a string')).toBe(false);
    expect(isRejection(undefined)).toBe(false);
    expect(isRejection(null)).toBe(false);
  });
});

// `NetworkFirst` (see vite.config.ts / sw-routes.ts) only falls back to the service worker's
// cache after a 4s network timeout, so a `Date` header set well over a minute before the
// request went out is the one signal available to the page that this happened.
describe('servedFromCache', () => {
  it('is false when there is no Date header at all', () => {
    expect(servedFromCache(null, Date.now())).toBe(false);
  });

  it('is false for a response dated moments before the request was sent -- ordinary latency', () => {
    const sentAt = Date.now();
    expect(servedFromCache(new Date(sentAt - 5_000).toUTCString(), sentAt)).toBe(false);
  });

  it('is true for a response dated well over a minute before the request was sent', () => {
    const sentAt = Date.now();
    expect(servedFromCache(new Date(sentAt - 61_000).toUTCString(), sentAt)).toBe(true);
  });

  it('is false for an unparsable Date header', () => {
    expect(servedFromCache('not a date', Date.now())).toBe(false);
  });

  // A self-hosted instance can have a server clock that is minutes off (no RTC, wrong timezone),
  // which would otherwise show the note permanently (server ahead) or hide a real cache hit
  // forever (server behind). `skewMs` is the server's known offset from this device's clock (see
  // `clockSkewMs` in ../src/lib/api.ts), subtracted before judging staleness.
  it('subtracts a steady clock skew before judging staleness', () => {
    const sentAt = Date.now();
    const skewMs = 65_000; // the server's clock reads 65s behind this device's
    // Genuinely fresh -- answered just now -- but its Date header alone would look well over a
    // minute old without correcting for the skew.
    expect(servedFromCache(new Date(sentAt - skewMs - 2_000).toUTCString(), sentAt, skewMs)).toBe(false);
    // Genuinely stale even after correcting for that very same skew.
    expect(servedFromCache(new Date(sentAt - skewMs - 65_000).toUTCString(), sentAt, skewMs)).toBe(true);
  });
});

describe('servingSaved', () => {
  // Each test starts from a clean slate regardless of what an earlier test in this file left
  // behind: staleness recorded for a path (or a calibrated clock skew) must never leak between
  // tests that otherwise look independent.
  beforeEach(() => {
    clearServingSaved();
    resetClockSkewForTesting();
  });

  it('flips true on a response served from the cache, and back on the next fresh one to the SAME path', async () => {
    const sentAt = Date.now();
    mockFetch(200, { items: [] }, new Date(sentAt - 61_000).toUTCString());
    await api('GET', '/objects');
    expect(get(servingSaved)).toBe(true);

    mockFetch(200, { items: [] }, new Date().toUTCString());
    await api('GET', '/objects');
    expect(get(servingSaved)).toBe(false);
  });

  it('a response with no Date header counts as fresh', async () => {
    mockFetch(200, { items: [] }, new Date(Date.now() - 61_000).toUTCString());
    await api('GET', '/objects');
    expect(get(servingSaved)).toBe(true);

    mockFetch(200, { items: [] }, null);
    await api('GET', '/objects');
    expect(get(servingSaved)).toBe(false);
  });

  // The bug this per-path Set replaced: a single "last response wins" flag flapped back to false
  // the instant ANY response came back fresh, even one that had nothing to do with the stale one
  // still visibly on screen (a faster sibling request on the same page, say).
  it('a fresh response to an unrelated path does not clear staleness recorded for another', async () => {
    const sentAt = Date.now();
    mockFetch(200, { items: [] }, new Date(sentAt - 61_000).toUTCString());
    await api('GET', '/objects');
    expect(get(servingSaved)).toBe(true);

    mockFetch(200, { items: [] }, new Date().toUTCString());
    await api('GET', '/activities'); // a different path entirely
    expect(get(servingSaved)).toBe(true); // /objects's staleness is untouched
  });

  it('clears on a route change, regardless of what is currently stale', async () => {
    const sentAt = Date.now();
    mockFetch(200, { items: [] }, new Date(sentAt - 61_000).toUTCString());
    await api('GET', '/objects');
    expect(get(servingSaved)).toBe(true);

    // What the router's `path` store changing calls in production (see the subscription in
    // ../src/lib/api.ts) -- exercised directly here since a real route change needs a DOM.
    clearServingSaved();
    expect(get(servingSaved)).toBe(false);
  });

  it('calibrates clock skew from an uncached auth/settings response before judging a cached path', async () => {
    const skewMs = 70_000; // the server's clock reads 70s behind this device's
    mockFetch(200, { id: 1 }, new Date(Date.now() - skewMs).toUTCString());
    await api('GET', '/auth/me'); // never cached -- calibrates the skew, is not itself judged

    // Genuinely fresh: its Date header alone would look well over a minute old, but only
    // because of the very same server clock skew just calibrated above.
    mockFetch(200, { items: [] }, new Date(Date.now() - skewMs - 2_000).toUTCString());
    await api('GET', '/objects');
    expect(get(servingSaved)).toBe(false);

    // Genuinely stale even after correcting for that same skew.
    mockFetch(200, { items: [] }, new Date(Date.now() - skewMs - 65_000).toUTCString());
    await api('GET', '/objects');
    expect(get(servingSaved)).toBe(true);
  });
});
