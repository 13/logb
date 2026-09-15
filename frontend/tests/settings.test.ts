import { describe, expect, it, afterEach } from 'vitest';
import { get } from 'svelte/store';

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

describe('settings', () => {
  afterEach(() => {
    // @ts-expect-error -- test-only cleanup of a global this suite installs
    delete globalThis.localStorage;
  });

  // `dateFormat` was added after `locale`/`theme` -- a device that saved its settings before
  // this shipped has neither field, and `persisted()` must fill it in from the default rather
  // than leaving it undefined (which `resolveDateFormat` was never written to accept).
  it('gives an older stored settings object with no dateFormat the "auto" default', async () => {
    const storage = installStorage();
    storage.setItem('logb.settings', JSON.stringify({ v: 1, data: { locale: 'de', theme: 'dark' } }));

    const { settings } = await import('../src/stores/settings');

    expect(get(settings)).toEqual({ locale: 'de', theme: 'dark', dateFormat: 'auto' });
  });
});
