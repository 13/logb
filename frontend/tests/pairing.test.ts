import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { createPairing } from '../src/lib/pairing';
import type { PairCode } from '../src/lib/types';

/** Same shape as `mockFetch` in tests/api.test.ts: a `Response` stub good enough for `api()`'s
 *  `handle()` to parse. */
function jsonResponse(status: number, body: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: (k: string) => (k.toLowerCase() === 'content-type' ? 'application/json' : null) },
    json: async () => body,
    text: async () => JSON.stringify(body),
  } as unknown as Response;
}

function mockFetch(status: number, body: unknown) {
  globalThis.fetch = vi.fn(async () => jsonResponse(status, body)) as unknown as typeof fetch;
}

function pairCode(over: Partial<PairCode> = {}): PairCode {
  return {
    code: 'c0de', uri: 'logb://pair?server=https%3A%2F%2Flogb.example%2F&code=c0de',
    qr_svg: '<svg data-testid="qr"><rect/></svg>', expires_at: new Date(Date.now() + 300_000).toISOString(),
    ...over,
  };
}

describe('createPairing', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('starts idle, offering the QR code', () => {
    const pairing = createPairing();
    expect(get(pairing.state)).toEqual({ phase: 'idle', pair: null, secondsLeft: 0 });
  });

  it('fetches a code from POST /auth/pair and shows it as a QR, counting down to expires_at', async () => {
    const pair = pairCode();
    mockFetch(201, pair);
    const pairing = createPairing();

    await pairing.request();

    const s = get(pairing.state);
    expect(s.phase).toBe('qr');
    expect(s.pair).toEqual(pair);
    expect(s.secondsLeft).toBe(300);
    const [url, init] = (globalThis.fetch as unknown as ReturnType<typeof vi.fn>).mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/auth/pair');
    expect(init.method).toBe('POST');

    pairing.stop();
  });

  it('ticks the countdown down once a second', async () => {
    mockFetch(201, pairCode({ expires_at: new Date(Date.now() + 10_000).toISOString() }));
    const pairing = createPairing();
    await pairing.request();

    expect(get(pairing.state).secondsLeft).toBe(10);
    await vi.advanceTimersByTimeAsync(3000);
    expect(get(pairing.state).secondsLeft).toBe(7);

    pairing.stop();
  });

  it('"show code instead" reveals the uri as plain text, and can be switched back', async () => {
    const pair = pairCode();
    mockFetch(201, pair);
    const pairing = createPairing();
    await pairing.request();

    pairing.showCode();
    expect(get(pairing.state).phase).toBe('code');
    expect(get(pairing.state).pair?.uri).toBe(pair.uri);

    pairing.showQr();
    expect(get(pairing.state).phase).toBe('qr');

    pairing.stop();
  });

  it('pressing the button again replaces the old code', async () => {
    const first = pairCode({ code: 'first' });
    mockFetch(201, first);
    const pairing = createPairing();
    await pairing.request();
    expect(get(pairing.state).pair?.code).toBe('first');

    const second = pairCode({ code: 'second' });
    mockFetch(201, second);
    await pairing.request();

    expect(get(pairing.state).phase).toBe('qr');
    expect(get(pairing.state).pair?.code).toBe('second');

    pairing.stop();
  });

  it('hides the QR and offers a new code once the countdown reaches zero', async () => {
    mockFetch(201, pairCode({ expires_at: new Date(Date.now() + 3000).toISOString() }));
    const pairing = createPairing();
    await pairing.request();
    expect(get(pairing.state).phase).toBe('qr');

    await vi.advanceTimersByTimeAsync(3000);

    const s = get(pairing.state);
    expect(s.phase).toBe('expired');
    expect(s.pair).toBeNull();
    expect(s.secondsLeft).toBe(0);
  });

  it('a failed request rejects and leaves the state untouched, for the caller to show the error', async () => {
    mockFetch(401, { error: 'unauthorized', message: 'authentication required' });
    const pairing = createPairing();

    await expect(pairing.request()).rejects.toThrow('authentication required');
    expect(get(pairing.state)).toEqual({ phase: 'idle', pair: null, secondsLeft: 0 });
  });
});
