import { describe, it, expect, vi, beforeEach } from 'vitest';
import { api, ApiError, setUnauthorizedHandler, fileUrl } from '../src/lib/api';

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
