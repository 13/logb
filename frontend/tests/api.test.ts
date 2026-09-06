import { describe, it, expect, vi, beforeEach } from 'vitest';
import { api, ApiError, isRejection, setUnauthorizedHandler, fileUrl } from '../src/lib/api';

function mockFetch(status: number, body: unknown) {
  const res = {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: (k: string) => (k.toLowerCase() === 'content-type' && body !== undefined ? 'application/json' : null) },
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
