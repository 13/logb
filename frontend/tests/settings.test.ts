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

  /** The four behaviours the account/device split has to keep, whatever its storage looks like. */
  async function load() {
    vi.resetModules();
    installStorage();
    return import('../src/stores/settings');
  }

  const ACCOUNT = { locale: 'de' as const, theme: 'dark' as const, dateFormat: 'iso' as const, firstDayOfWeek: 'monday' as const };

  it('lets a preference load that started before a local edit lose to that edit', async () => {
    const { settings, appearanceOwner, applyAccountAppearance, appearanceSnapshot } = await load();
    appearanceOwner(1);
    const inFlight = appearanceSnapshot();
    settings.update(s => ({ ...s, firstDayOfWeek: 'sunday' }));
    applyAccountAppearance(1, ACCOUNT, inFlight);
    expect(get(settings).firstDayOfWeek).toBe('sunday');
    // The account value is still remembered: the next device that loads it gets it.
    appearanceOwner(null);
    appearanceOwner(1);
    expect(get(settings).firstDayOfWeek).toBe('sunday');
  });

  it('applies a preference load that nothing has raced', async () => {
    const { settings, appearanceOwner, applyAccountAppearance, appearanceSnapshot } = await load();
    appearanceOwner(1);
    applyAccountAppearance(1, ACCOUNT, appearanceSnapshot());
    expect(get(settings)).toEqual(ACCOUNT);
  });

  it('keeps a device override across a sign-out and back in', async () => {
    const { settings, appearanceOwner, applyAccountAppearance, setDeviceOverride } = await load();
    appearanceOwner(1);
    setDeviceOverride(1, true);
    settings.update(s => ({ ...s, theme: 'light' }));
    appearanceOwner(null);
    appearanceOwner(1);
    applyAccountAppearance(1, ACCOUNT);
    expect(get(settings).theme).toBe('light');
  });

  it('does not leak one account\'s appearance into another on the same device', async () => {
    const { settings, appearanceOwner, applyAccountAppearance } = await load();
    appearanceOwner(1);
    applyAccountAppearance(1, ACCOUNT);
    appearanceOwner(2);
    expect(get(settings).locale).toBe('auto');
    // A load that arrives for the account that just left changes nothing for the one here now.
    applyAccountAppearance(1, ACCOUNT);
    expect(get(settings).locale).toBe('auto');
    appearanceOwner(1);
    expect(get(settings).locale).toBe('de');
  });

  it('adopts what a fresh device was already set to when its first account signs in', async () => {
    const { settings, appearanceOwner } = await load();
    settings.update(s => ({ ...s, theme: 'dark' }));
    appearanceOwner(7);
    expect(get(settings).theme).toBe('dark');
  });
});
