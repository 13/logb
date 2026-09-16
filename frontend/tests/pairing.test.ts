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

/** A promise this test resolves by hand, so two overlapping `request()` calls can be made to
 *  answer in a chosen order instead of whichever order two real `await`s would happen to. */
function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => { resolve = r; });
  return { promise, resolve };
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

  it('a double click leaves exactly one live interval, whichever response arrives first', async () => {
    const first = pairCode({ code: 'first' });
    const second = pairCode({ code: 'second' });
    const firstResponse = deferred<Response>();
    const secondResponse = deferred<Response>();
    let calls = 0;
    globalThis.fetch = vi.fn(
      () => (++calls === 1 ? firstResponse.promise : secondResponse.promise),
    ) as unknown as typeof fetch;

    const pairing = createPairing();
    const clickA = pairing.request();
    const clickB = pairing.request();
    expect(calls).toBe(2); // both clicks reached the network before either answered

    // The second click's response -- the newer one -- arrives first, as a fast retry racing a
    // slow first attempt would.
    secondResponse.resolve(jsonResponse(201, second));
    await clickB;
    expect(get(pairing.state).pair?.code).toBe('second');
    expect(vi.getTimerCount()).toBe(1);

    // The first click's response arrives late. Before the fix this replaced the state above
    // and started a second, uncleared interval.
    firstResponse.resolve(jsonResponse(201, first));
    await clickA;

    expect(get(pairing.state).pair?.code).toBe('second');
    expect(vi.getTimerCount()).toBe(1);

    pairing.stop();
  });

  it('an older response arriving after a newer one does not replace or hide it', async () => {
    const older = pairCode({ code: 'older', expires_at: new Date(Date.now() + 60_000).toISOString() });
    const newer = pairCode({ code: 'newer', expires_at: new Date(Date.now() + 120_000).toISOString() });
    const olderResponse = deferred<Response>();
    const newerResponse = deferred<Response>();
    let calls = 0;
    globalThis.fetch = vi.fn(
      () => (++calls === 1 ? olderResponse.promise : newerResponse.promise),
    ) as unknown as typeof fetch;

    const pairing = createPairing();
    const first = pairing.request();
    const second = pairing.request();

    newerResponse.resolve(jsonResponse(201, newer));
    await second;
    const afterNewer = get(pairing.state);
    expect(afterNewer.phase).toBe('qr');
    expect(afterNewer.pair?.code).toBe('newer');

    olderResponse.resolve(jsonResponse(201, older));
    await first;

    // Neither replaced (the older code showing) nor hidden (falling back to idle/expired).
    const afterOlder = get(pairing.state);
    expect(afterOlder.phase).toBe('qr');
    expect(afterOlder.pair?.code).toBe('newer');

    pairing.stop();
  });

  it('a response that arrives after stop() (as onDestroy calls it) never starts an interval', async () => {
    const pending = deferred<Response>();
    globalThis.fetch = vi.fn(() => pending.promise) as unknown as typeof fetch;

    const pairing = createPairing();
    const inFlight = pairing.request();
    pairing.stop(); // stands in for a component's onDestroy firing mid-request

    pending.resolve(jsonResponse(201, pairCode()));
    await inFlight;

    expect(get(pairing.state)).toEqual({ phase: 'idle', pair: null, secondsLeft: 0 });
    expect(vi.getTimerCount()).toBe(0);
  });
});
