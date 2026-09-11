import { afterEach, describe, expect, it, vi } from 'vitest';
import { clearObjectCache, getCachedObject, setCachedObject } from '../src/lib/object-cache';
import type { MemObject } from '../src/lib/types';

const obj = { id: 1, name: 'Golf' } as unknown as MemObject;

describe('clearObjectCache', () => {
  afterEach(() => {
    // @ts-expect-error -- test-only cleanup of a global this suite installs
    delete globalThis.caches;
  });

  it('drops the in-memory Maps', () => {
    setCachedObject(1, obj);
    clearObjectCache();
    expect(getCachedObject(1)).toBeUndefined();
  });

  it('deletes exactly the two named service-worker caches vite.config.ts configures', async () => {
    const del = vi.fn().mockResolvedValue(true);
    globalThis.caches = { delete: del } as unknown as CacheStorage;

    clearObjectCache();
    // `caches.delete` is called synchronously even though it returns a promise -- give any
    // microtask queued by the (unawaited) call a turn before asserting.
    await Promise.resolve();

    expect(del).toHaveBeenCalledWith('logb-api');
    expect(del).toHaveBeenCalledWith('logb-files');
    expect(del).toHaveBeenCalledTimes(2);
  });

  it('does not throw when the Cache API does not exist (vitest, older browsers)', () => {
    // @ts-expect-error -- deliberately absent, as in the node test environment by default
    delete globalThis.caches;
    setCachedObject(1, obj);
    expect(() => clearObjectCache()).not.toThrow();
  });
});
