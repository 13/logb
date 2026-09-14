import { describe, expect, it } from 'vitest';
import { claimCaches, forgetCacheOwner, forgetProfile, rememberProfile, rememberedProfile } from '../src/lib/cache-owner';

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

describe('claimCaches', () => {
  it('asks for a clear when no owner is recorded, and records the claimant', () => {
    const s = memoryStorage();
    expect(claimCaches(7, s)).toBe(true);
    expect(s.getItem('logb.cache.user')).toBe('7');
  });

  it('keeps the caches for the same user', () => {
    const s = memoryStorage();
    claimCaches(7, s);
    expect(claimCaches(7, s)).toBe(false);
  });

  it('asks for a clear when someone else owns them, and records the new owner', () => {
    const s = memoryStorage();
    claimCaches(7, s);
    expect(claimCaches(8, s)).toBe(true);
    expect(s.getItem('logb.cache.user')).toBe('8');
    expect(claimCaches(8, s)).toBe(false);
  });

  it('asks for a clear again once the owner is forgotten', () => {
    const s = memoryStorage();
    claimCaches(7, s);
    forgetCacheOwner(s);
    expect(s.getItem('logb.cache.user')).toBeNull();
    expect(claimCaches(7, s)).toBe(true);
  });
});

describe('storage that throws (private mode)', () => {
  it('remembers nothing, reads no profile, and always clears to be safe', () => {
    const s = throwingStorage();
    expect(() => rememberProfile(BEN, s)).not.toThrow();
    expect(rememberedProfile(s)).toBeNull();
    expect(claimCaches(7, s)).toBe(true);
    expect(claimCaches(7, s)).toBe(true);
    expect(() => forgetProfile(s)).not.toThrow();
    expect(() => forgetCacheOwner(s)).not.toThrow();
  });
});
