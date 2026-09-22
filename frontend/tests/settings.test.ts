import { describe, expect, it, afterEach, vi } from 'vitest';
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

    expect(get(settings)).toEqual({ locale: 'de', theme: 'dark', dateFormat: 'auto', firstDayOfWeek: 'locale' });
  });

  it('isolates accounts, honors device overrides and ignores stale preference loads', async () => {
    vi.resetModules(); installStorage();
    const { settings, appearanceOwner, applyAccountAppearance, appearanceRevision, deviceOverride } = await import('../src/stores/settings');
    appearanceOwner(1);
    const account = { locale: 'de' as const, theme: 'dark' as const, dateFormat: 'iso' as const, firstDayOfWeek: 'monday' as const };
    applyAccountAppearance(1, account);
    const beforeEdit = appearanceRevision();
    settings.update(s => ({ ...s, firstDayOfWeek: 'sunday' }));
    applyAccountAppearance(1, account, beforeEdit);
    expect(get(settings).firstDayOfWeek).toBe('sunday');
    deviceOverride.set({ '1': true });
    applyAccountAppearance(1, account);
    expect(get(settings).firstDayOfWeek).toBe('sunday');
    appearanceOwner(2);
    expect(get(settings).locale).toBe('auto');
    applyAccountAppearance(1, account);
    expect(get(settings).locale).toBe('auto');
    appearanceOwner(1);
    expect(get(settings).firstDayOfWeek).toBe('sunday');
  });
});
