import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const apiMock = vi.hoisted(() => vi.fn());
vi.mock('../src/lib/api', () => ({ api: apiMock }));

import {
  CUSTOM_TYPE_ICONS, categoriesFor, clearCustomTypes, customTypes, defaultUnit, loadCustomTypes, typeIcon, typeLabel,
  typesLoaded,
} from '../src/lib/type-registry';
import { CATEGORIES, OBJECT_TYPES, type CustomType } from '../src/lib/types';

const t = (k: string) => `T(${k})`;

const scooter: CustomType = {
  id: 7, client_uuid: '1b4e28ba-2fa1-11d2-883f-0016d3cca427', key: 'custom:1b4e28ba-2fa1-11d2-883f-0016d3cca427',
  name: 'E-scooter', icon: 'e-bike', categories: ['maintenance', 'repair', 'other'], counter_unit: 'km',
  created_at: '2026-09-14T10:00:00Z', updated_at: '2026-09-14T10:00:00Z',
};
const custom = [scooter];

describe('type registry: built-in types', () => {
  // The table moved here from object-types.ts; these are the values it had, so the move cannot
  // have changed what any existing object shows or offers.
  it('keeps every built-in icon and category list as they were', () => {
    const icons = Object.fromEntries(OBJECT_TYPES.map((k) => [k, typeIcon(k, custom)]));
    expect(icons).toEqual({
      car: 'car', e_bike: 'e-bike', bike: 'bike', motorcycle: 'motorcycle', home: 'home',
      appliance: 'appliance', tool: 'tool', body: 'body', other: 'object',
    });
    const vehicle = ['maintenance', 'repair', 'inspection', 'fuel', 'reading', 'modification', 'purchase', 'other'];
    expect(categoriesFor('car', custom)).toEqual(vehicle);
    expect(categoriesFor('e_bike', custom)).toEqual(vehicle);
    expect(categoriesFor('motorcycle', custom)).toEqual(vehicle);
    expect(categoriesFor('bike', custom)).toEqual(['maintenance', 'repair', 'inspection', 'reading', 'modification', 'purchase', 'other']);
    expect(categoriesFor('home', custom)).toEqual(['maintenance', 'repair', 'inspection', 'modification', 'purchase', 'session', 'other']);
    expect(categoriesFor('appliance', custom)).toEqual(['maintenance', 'repair', 'inspection', 'reading', 'modification', 'purchase', 'other']);
    expect(categoriesFor('tool', custom)).toEqual(['maintenance', 'repair', 'inspection', 'reading', 'modification', 'purchase', 'other']);
    expect(categoriesFor('body', custom)).toEqual(['weight', 'symptom', 'treatment', 'appointment', 'medication', 'other']);
    expect(categoriesFor('other', custom)).toEqual([...CATEGORIES]);
  });

  it('labels a built-in type through its translation key', () => {
    expect(typeLabel('car', custom, t, true)).toBe('T(type.car)');
    expect(typeLabel('other', [], t, true)).toBe('T(type.other)');
  });

  it('gives built-in types no default unit', () => {
    expect(defaultUnit('car', custom)).toBeNull();
  });
});

describe('type registry: own types', () => {
  it('resolves an own type to its name, icon, categories and unit', () => {
    expect(typeLabel(scooter.key, custom, t, true)).toBe('E-scooter');
    expect(typeIcon(scooter.key, custom)).toBe('e-bike');
    expect(categoriesFor(scooter.key, custom)).toEqual(['maintenance', 'repair', 'other']);
    expect(defaultUnit(scooter.key, custom)).toBe('km');
  });

  // Same rule as for built-in types: an entry filed before its object changed type keeps its
  // own category in the select.
  it('keeps the current category, once', () => {
    expect(categoriesFor(scooter.key, custom, 'fuel')).toEqual(['maintenance', 'repair', 'other', 'fuel']);
    expect(categoriesFor(scooter.key, custom, 'repair').filter((c) => c === 'repair')).toHaveLength(1);
  });

  // A type deleted on another device, or not synced here yet: the object still has to render,
  // and offering every category is the only choice that cannot hide an entry's own.
  it('treats an own type missing from a loaded list as unknown', () => {
    const gone = 'custom:00000000-0000-4000-8000-000000000000';
    expect(typeLabel(gone, custom, t, true)).toBe('T(types.unknown)');
    expect(typeLabel(gone, custom, t, false)).toBe('T(types.loading)');
    expect(typeIcon(gone, custom)).toBe('object');
    expect(categoriesFor(gone, custom)).toEqual([...CATEGORIES]);
    expect(defaultUnit(gone, custom)).toBeNull();
  });

  it('does not let a custom type shadow a built-in key', () => {
    const odd = { ...scooter, key: 'car', name: 'Not a car' };
    expect(typeLabel('car', [odd], t, true)).toBe('T(type.car)');
  });

  it('offers no UI-only icon for a type', () => {
    for (const icon of ['back', 'settings', 'search', 'edit', 'plus', 'chevron', 'logout']) {
      expect(CUSTOM_TYPE_ICONS as string[]).not.toContain(icon);
    }
  });

  // `box` and `object` draw nearly the same cube; offering both is two identical picker choices.
  it('offers the cube once, not also as a box', () => {
    expect(CUSTOM_TYPE_ICONS).toContain('object');
    expect(CUSTOM_TYPE_ICONS as string[]).not.toContain('box');
  });
});

describe('type registry: loading, storage and users', () => {
  const gone = 'custom:00000000-0000-4000-8000-000000000000';
  const label = (key: string) => typeLabel(key, get(customTypes), t, get(typesLoaded));
  let storage: Map<string, string>;

  beforeEach(() => {
    storage = new Map();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => storage.get(k) ?? null,
      setItem: (k: string, v: string) => void storage.set(k, v),
      removeItem: (k: string) => void storage.delete(k),
    });
    clearCustomTypes();
    apiMock.mockReset();
  });

  it('says nothing definite about an own type before the list has loaded', () => {
    expect(get(typesLoaded)).toBe(false);
    expect(label(gone)).toBe('T(types.loading)');
  });

  it('calls a key unknown once the list has loaded without it, and stores the list', async () => {
    apiMock.mockResolvedValue([scooter]);
    await loadCustomTypes(1);
    expect(get(typesLoaded)).toBe(true);
    expect(label(scooter.key)).toBe('E-scooter');
    expect(label(gone)).toBe('T(types.unknown)');
    expect(JSON.parse(storage.get('logb.types.1')!)).toEqual([scooter]);
  });

  it('uses the stored list before the network answers', () => {
    storage.set('logb.types.2', JSON.stringify([scooter]));
    apiMock.mockReturnValue(new Promise(() => {}));
    void loadCustomTypes(2);
    expect(get(typesLoaded)).toBe(true);
    expect(label(scooter.key)).toBe('E-scooter');
  });

  it('keeps the stored list when the network fails', async () => {
    storage.set('logb.types.2', JSON.stringify([scooter]));
    apiMock.mockRejectedValue(new TypeError('offline'));
    await loadCustomTypes(2);
    expect(label(scooter.key)).toBe('E-scooter');
  });

  it("never shows the previous user's types to the next one", async () => {
    apiMock.mockResolvedValue([scooter]);
    await loadCustomTypes(1);
    let answerFirst: (v: unknown) => void = () => {};
    apiMock.mockReturnValueOnce(new Promise((r) => { answerFirst = r; }));
    const late = loadCustomTypes(1); // still on the wire when the account changes
    apiMock.mockReturnValue(new Promise(() => {}));
    void loadCustomTypes(3);
    expect(get(customTypes)).toEqual([]);
    expect(get(typesLoaded)).toBe(false);
    answerFirst([scooter]);
    await late;
    expect(get(customTypes)).toEqual([]);
    expect(label(scooter.key)).toBe('T(types.loading)');
  });

  it('forgets the stored list when the session ends', async () => {
    apiMock.mockResolvedValue([scooter]);
    await loadCustomTypes(1);
    clearCustomTypes();
    expect(storage.has('logb.types.1')).toBe(false);
    expect(get(customTypes)).toEqual([]);
    expect(get(typesLoaded)).toBe(false);
  });

  it('works without storage at all', async () => {
    vi.stubGlobal('localStorage', { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); }, removeItem: () => { throw new Error('blocked'); } });
    apiMock.mockResolvedValue([scooter]);
    await loadCustomTypes(4);
    expect(label(scooter.key)).toBe('E-scooter');
    expect(() => clearCustomTypes()).not.toThrow();
  });
});
