import { describe, expect, it } from 'vitest';
import { cachesBelongTo, forgetCacheOwner, forgetProfile, recordCacheOwner, rememberProfile, rememberedProfile, userSwitchNeedsReload } from '../src/lib/cache-owner';

/** An in-memory `Storage`, so the module is tested without a browser. */
function memoryStorage(): Storage {
  const m = new Map<string, string>();
  return {
    get length() { return m.size; },
    clear: () => m.clear(),
    getItem: (k: string) => m.get(k) ?? null,
    key: (i: number) => [...m.keys()][i] ?? null,
    removeItem: (k: string) => { m.delete(k); },
    setItem: (k: string, v: string) => { m.set(k, String(v)); },
  };
}

/** What private browsing (or blocked site data) looks like: every access throws. */
function throwingStorage(): Storage {
  const no = () => { throw new DOMException('blocked', 'SecurityError'); };
  return { length: 0, clear: no, getItem: no, key: no, removeItem: no, setItem: no } as unknown as Storage;
}

const BEN = { id: 7, username: 'ben', is_admin: true, lang: 'de' };

describe('remembered profile', () => {
  it('reads back what was remembered, and nothing once forgotten', () => {
    const s = memoryStorage();
    expect(rememberedProfile(s)).toBeNull();
    rememberProfile(BEN, s);
    expect(rememberedProfile(s)).toEqual(BEN);
    forgetProfile(s);
    expect(rememberedProfile(s)).toBeNull();
  });

  it('keeps only the profile fields, under logb.session.profile', () => {
    const s = memoryStorage();
    rememberProfile({ ...BEN, created_at: '2026-01-01' } as typeof BEN, s);
    expect(JSON.parse(s.getItem('logb.session.profile')!)).toEqual(BEN);
  });

  it('treats malformed JSON as no profile', () => {
    const s = memoryStorage();
    s.setItem('logb.session.profile', '{not json');
    expect(rememberedProfile(s)).toBeNull();
  });

  it('treats a profile with missing or mistyped fields as no profile', () => {
    const s = memoryStorage();
    s.setItem('logb.session.profile', JSON.stringify({ id: 7, username: 'ben', is_admin: true }));
    expect(rememberedProfile(s)).toBeNull();
    s.setItem('logb.session.profile', JSON.stringify({ ...BEN, id: '7' }));
    expect(rememberedProfile(s)).toBeNull();
    s.setItem('logb.session.profile', JSON.stringify(null));
    expect(rememberedProfile(s)).toBeNull();
  });
});

describe('cache owner', () => {
  it('does not count as the owner when none is recorded, and checking records nothing', () => {
    const s = memoryStorage();
    expect(cachesBelongTo(7, s)).toBe(false);
    expect(s.getItem('logb.cache.user')).toBeNull();
  });

  it('belongs to the recorded user only', () => {
    const s = memoryStorage();
    recordCacheOwner(7, s);
    expect(s.getItem('logb.cache.user')).toBe('7');
    expect(cachesBelongTo(7, s)).toBe(true);
    expect(cachesBelongTo(8, s)).toBe(false);
    recordCacheOwner(8, s);
    expect(cachesBelongTo(8, s)).toBe(true);
    expect(cachesBelongTo(7, s)).toBe(false);
  });

  it('belongs to nobody again once the owner is forgotten', () => {
    const s = memoryStorage();
    recordCacheOwner(7, s);
    forgetCacheOwner(s);
    expect(s.getItem('logb.cache.user')).toBeNull();
    expect(cachesBelongTo(7, s)).toBe(false);
  });
});

describe('userSwitchNeedsReload', () => {
  const profile = (id: number, lang = 'de') => JSON.stringify({ id, username: `u${id}`, is_admin: false, lang });

  it('reloads when another tab records a different cache owner or profile', () => {
    expect(userSwitchNeedsReload('logb.cache.user', '7', '8', 7)).toBe(true);
    expect(userSwitchNeedsReload('logb.session.profile', profile(7), profile(8), 7)).toBe(true);
  });

  it('reloads when another tab removes them, or clears storage', () => {
    expect(userSwitchNeedsReload('logb.cache.user', '7', null, 7)).toBe(true);
    expect(userSwitchNeedsReload('logb.session.profile', profile(7), null, 7)).toBe(true);
    expect(userSwitchNeedsReload(null, null, null, 7)).toBe(true);
  });

  it('does not reload for the user this tab already holds', () => {
    expect(userSwitchNeedsReload('logb.cache.user', null, '7', 7)).toBe(false);
    expect(userSwitchNeedsReload('logb.session.profile', null, profile(7), 7)).toBe(false);
    // The same person's language changing in another tab.
    expect(userSwitchNeedsReload('logb.session.profile', profile(7, 'de'), profile(7, 'en'), 7)).toBe(false);
  });

  it('does not reload for an unchanged value, another key, or a tab holding no user', () => {
    expect(userSwitchNeedsReload('logb.cache.user', '8', '8', 7)).toBe(false);
    expect(userSwitchNeedsReload('logb.outbox', 'a', 'b', 7)).toBe(false);
    expect(userSwitchNeedsReload('logb.cache.user', '7', '8', null)).toBe(false);
    expect(userSwitchNeedsReload('logb.cache.user', '7', '8', undefined)).toBe(false);
  });

  it('treats an unreadable value as not this tab\'s user', () => {
    expect(userSwitchNeedsReload('logb.session.profile', profile(7), '{not json', 7)).toBe(true);
    expect(userSwitchNeedsReload('logb.cache.user', '7', 'x', 7)).toBe(true);
  });
});

describe('storage that throws (private mode)', () => {
  it('remembers nothing, reads no profile, and always clears to be safe', () => {
    const s = throwingStorage();
    expect(() => rememberProfile(BEN, s)).not.toThrow();
    expect(rememberedProfile(s)).toBeNull();
    recordCacheOwner(7, s);
    expect(cachesBelongTo(7, s)).toBe(false);
    expect(() => forgetProfile(s)).not.toThrow();
    expect(() => forgetCacheOwner(s)).not.toThrow();
  });
});
