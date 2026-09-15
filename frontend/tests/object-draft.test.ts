import { beforeEach, describe, expect, it, vi } from 'vitest';
import { forgetObjectDraft, safeReturnPath, saveObjectDraft, takeObjectDraft } from '../src/lib/object-draft';
import type { ObjectInput } from '../src/lib/types';

describe('object-draft: safeReturnPath', () => {
  it('accepts /objects/new', () => {
    expect(safeReturnPath('/objects/new')).toBe('/objects/new');
  });

  it('accepts /objects/:id/edit', () => {
    expect(safeReturnPath('/objects/12/edit')).toBe('/objects/12/edit');
  });

  it('refuses an absolute URL disguised as a path', () => {
    expect(safeReturnPath('https://evil.example/objects/new')).toBeNull();
  });

  it('refuses a protocol-relative address', () => {
    expect(safeReturnPath('//evil/objects')).toBeNull();
  });

  it('refuses a path outside the object form', () => {
    expect(safeReturnPath('/settings')).toBeNull();
  });

  it('refuses null', () => {
    expect(safeReturnPath(null)).toBeNull();
  });

  it('refuses a path with traversal segments', () => {
    expect(safeReturnPath('/objects/../settings')).toBeNull();
  });

  it('refuses raw percent-encoding, undecoded, disguised as a path', () => {
    expect(safeReturnPath('%2F%2Fevil')).toBeNull();
  });

  it('refuses a path with a trailing query string', () => {
    expect(safeReturnPath('/objects/new?x')).toBeNull();
  });

  it('refuses a path with a trailing slash', () => {
    expect(safeReturnPath('/objects/new/')).toBeNull();
  });

  it('refuses a backslash-led address (some browsers treat \\ as /)', () => {
    expect(safeReturnPath('/\\evil')).toBeNull();
  });

  it('refuses a double-backslash address', () => {
    expect(safeReturnPath('\\\\evil')).toBeNull();
  });
});

describe('object-draft: save and take', () => {
  let storage: Map<string, string>;

  beforeEach(() => {
    storage = new Map();
    vi.stubGlobal('sessionStorage', {
      getItem: (k: string) => storage.get(k) ?? null,
      setItem: (k: string, v: string) => void storage.set(k, v),
      removeItem: (k: string) => void storage.delete(k),
    });
  });

  const input: ObjectInput = {
    name: 'Mein Pedelec', type: 'other', counter_unit: null, fuel_unit: null, description: '',
    purchase_date: null, purchase_price_cents: null, archived: false, parent_id: null, tags: [],
  };

  it('returns what was saved for the same path, under the token it was saved with', () => {
    const token = saveObjectDraft('/objects/new', input);
    expect(takeObjectDraft('/objects/new', token)).toEqual(input);
  });

  it('is used at most once, even with the right token', () => {
    const token = saveObjectDraft('/objects/new', input);
    takeObjectDraft('/objects/new', token);
    expect(takeObjectDraft('/objects/new', token)).toBeNull();
  });

  it('returns null for a different path than it was saved under', () => {
    const token = saveObjectDraft('/objects/new', input);
    expect(takeObjectDraft('/objects/12/edit', token)).toBeNull();
  });

  it('returns null when the stored JSON is unreadable', () => {
    storage.set('logb.object-draft', '{not json');
    expect(takeObjectDraft('/objects/new', 'anything')).toBeNull();
  });

  it('returns null, and discards the draft, when no token is given', () => {
    const token = saveObjectDraft('/objects/new', input);
    expect(takeObjectDraft('/objects/new', null)).toBeNull();
    // The mismatched read above must not have left the draft sitting there for a later, correct
    // read to still pick up -- a plain visit with no token at all discards it outright.
    expect(takeObjectDraft('/objects/new', token)).toBeNull();
  });

  it('returns null, and discards the draft, when the token is wrong', () => {
    saveObjectDraft('/objects/new', input);
    expect(takeObjectDraft('/objects/new', 'not-the-token')).toBeNull();
    expect(storage.has('logb.object-draft')).toBe(false);
  });

  it('forgetObjectDraft clears a stored draft outright', () => {
    const token = saveObjectDraft('/objects/new', input);
    forgetObjectDraft();
    expect(takeObjectDraft('/objects/new', token)).toBeNull();
  });

  it('forgetObjectDraft is a no-op when nothing is stored', () => {
    expect(() => forgetObjectDraft()).not.toThrow();
  });
});
