import { beforeEach, describe, expect, it, vi } from 'vitest';
import { safeReturnPath, saveObjectDraft, takeObjectDraft } from '../src/lib/object-draft';
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

  it('returns what was saved for the same path', () => {
    saveObjectDraft('/objects/new', input);
    expect(takeObjectDraft('/objects/new')).toEqual(input);
  });

  it('is used at most once', () => {
    saveObjectDraft('/objects/new', input);
    takeObjectDraft('/objects/new');
    expect(takeObjectDraft('/objects/new')).toBeNull();
  });

  it('returns null for a different path than it was saved under', () => {
    saveObjectDraft('/objects/new', input);
    expect(takeObjectDraft('/objects/12/edit')).toBeNull();
  });

  it('returns null when the stored JSON is unreadable', () => {
    storage.set('logb.object-draft', '{not json');
    expect(takeObjectDraft('/objects/new')).toBeNull();
  });
});
