import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, errorMessage } from '../src/lib/api-error';
import en from '../src/i18n/en';

/** The real English dictionary, with the store's own "missing key renders the key" rule. */
const t = (key: string, vars?: Record<string, string | number>) => {
  let s = (en as Record<string, string>)[key] ?? key;
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, String(v));
  return s;
};

describe('errorMessage', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('says "no connection" for a fetch that never reached the server', () => {
    expect(errorMessage(new TypeError('Failed to fetch'), t)).toBe(en['error.offline']);
    expect(errorMessage(new TypeError('NetworkError when attempting to fetch resource.'), t)).toBe(en['error.offline']);
    expect(errorMessage(new TypeError('Load failed'), t)).toBe(en['error.offline']);
  });

  it('says "no connection" for any non-server failure while the browser is offline', () => {
    vi.stubGlobal('navigator', { onLine: false });
    expect(errorMessage(new Error('whatever the browser threw'), t)).toBe(en['error.offline']);
  });

  it('does not mistake an ordinary TypeError for a network failure', () => {
    expect(errorMessage(new TypeError("Cannot read properties of undefined (reading 'x')"), t))
      .toBe("Cannot read properties of undefined (reading 'x')");
  });

  it('never overrides what the server actually said with "offline"', () => {
    vi.stubGlobal('navigator', { onLine: false });
    expect(errorMessage(new ApiError(400, 'bad_request', 'date must be YYYY-MM-DD'), t)).toBe('date must be YYYY-MM-DD');
  });

  it('translates a known stable server code', () => {
    expect(errorMessage(new ApiError(404, 'not_found', 'not found'), t)).toBe(en['error.not-found']);
    expect(errorMessage(new ApiError(403, 'forbidden', 'forbidden'), t)).toBe(en['error.forbidden']);
    expect(errorMessage(new ApiError(413, 'too_large', 'payload too large'), t)).toBe(en['error.too-large']);
    expect(errorMessage(new ApiError(429, 'too_many_requests', 'too many requests'), t)).toBe(en['error.too-many-requests']);
    expect(errorMessage(new ApiError(500, 'internal', 'internal error'), t)).toBe(en['error.internal']);
    expect(errorMessage(new ApiError(400, 'name_taken', 'name taken'), t)).toBe(en['types.error.name_taken']);
  });

  it('translates the busy answer but keeps any other "unavailable" sentence', () => {
    expect(errorMessage(new ApiError(503, 'unavailable', 'the database is busy, please retry'), t)).toBe(en['error.busy']);
    expect(errorMessage(new ApiError(503, 'unavailable', 'save a Telegram bot token first'), t)).toBe('save a Telegram bot token first');
  });

  it("keeps the server's own sentence for a code whose message carries the detail", () => {
    expect(errorMessage(new ApiError(400, 'bad_request', 'name must not be empty'), t)).toBe('name must not be empty');
    expect(errorMessage(new ApiError(409, 'conflict', 'username already exists'), t)).toBe('username already exists');
  });

  it('translates a message that is an i18n key', () => {
    expect(errorMessage(new Error('outbox.queue-failed'), t)).toBe(en['outbox.queue-failed']);
    expect(errorMessage(new Error('object.pending-lost'), t)).toBe(en['object.pending-lost']);
  });

  it('passes a plain sentence through, and never shows an empty string', () => {
    expect(errorMessage(new Error('Check the name: required'), t)).toBe('Check the name: required');
    expect(errorMessage(new Error(''), t)).toBe(en['error.generic']);
    expect(errorMessage('boom', t)).toBe('boom');
    expect(errorMessage(undefined, t)).toBe(en['error.generic']);
  });
});
