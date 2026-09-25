import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { forgetObjectTemplates, loadObjectTemplates, removeObjectTemplate, saveObjectTemplate } from '../src/lib/object-templates';
import { emptyInput } from '../src/lib/object-form';

/** An in-memory `localStorage`: vitest runs in node, which has none. `full` makes every write
 *  throw the way a browser does when the quota is used up. */
function installStorage() {
  const m = new Map<string, string>();
  const state = { full: false };
  const s = {
    get length() { return m.size; },
    clear: () => m.clear(),
    getItem: (k: string) => m.get(k) ?? null,
    key: (i: number) => [...m.keys()][i] ?? null,
    removeItem: (k: string) => { m.delete(k); },
    setItem: (k: string, v: string) => {
      if (state.full) throw new DOMException('quota', 'QuotaExceededError');
      m.set(k, String(v));
    },
  } as Storage;
  Object.defineProperty(globalThis, 'localStorage', { value: s, configurable: true });
  return { map: m, state };
}

const input = () => ({ ...emptyInput(), name: 'Bike', type: 'car' as const, counter_unit: 'km' as const, tags: ['Summer'],
  parent_id: 12, private: true, archived: true });

describe('object templates', () => {
  let storage: ReturnType<typeof installStorage>;
  beforeEach(() => { storage = installStorage(); });
  afterEach(() => { delete (globalThis as { localStorage?: Storage }).localStorage; });

  it("keeps each user's templates apart", () => {
    saveObjectTemplate(1, input());
    expect(loadObjectTemplates(1).map((t) => t.name)).toEqual(['Bike']);
    expect(loadObjectTemplates(2)).toEqual([]);
  });

  it('does not keep where the object sat, or whether it was private or archived', () => {
    const [saved] = saveObjectTemplate(1, input());
    expect(saved.input.parent_id).toBeNull();
    expect(saved.input.private).toBe(false);
    expect(saved.input.archived).toBe(false);
    expect(saved.input.counter_unit).toBe('km');
    expect(saved.input.tags).toEqual(['Summer']);
    expect(loadObjectTemplates(1)[0].input.parent_id).toBeNull();
  });

  it('strips those fields from a template stored before they were stripped', () => {
    storage.map.set('logb.object-templates.1', JSON.stringify([{ id: 'a', name: 'Old', input: input() }]));
    const [old] = loadObjectTemplates(1);
    expect(old.input).toMatchObject({ parent_id: null, private: false, archived: false });
  });

  it('replaces a template of the same name, case-insensitively, and removes by id', () => {
    saveObjectTemplate(1, input());
    const list = saveObjectTemplate(1, { ...input(), name: 'bike' });
    expect(list.map((t) => t.name)).toEqual(['bike']);
    expect(removeObjectTemplate(1, list[0].id)).toEqual([]);
    expect(loadObjectTemplates(1)).toEqual([]);
  });

  it('survives a full storage: nothing throws, and the list says what was actually kept', () => {
    saveObjectTemplate(1, input());
    storage.state.full = true;
    const list = saveObjectTemplate(1, { ...input(), name: 'Car' });
    expect(list.map((t) => t.name)).toEqual(['Bike']);
    expect(removeObjectTemplate(1, list[0].id).map((t) => t.name)).toEqual(['Bike']);
  });

  it('forgets every user\'s templates, and the old shared list too', () => {
    storage.map.set('logb.object-templates', '[]');
    saveObjectTemplate(1, input());
    saveObjectTemplate(2, input());
    storage.map.set('logb.other', 'x');
    forgetObjectTemplates();
    expect([...storage.map.keys()]).toEqual(['logb.other']);
  });

  it('reads nothing without a user or without storage', () => {
    expect(loadObjectTemplates(null)).toEqual([]);
    delete (globalThis as { localStorage?: Storage }).localStorage;
    expect(loadObjectTemplates(1)).toEqual([]);
    expect(() => forgetObjectTemplates()).not.toThrow();
  });
});
