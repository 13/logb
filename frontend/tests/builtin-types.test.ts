import { describe, expect, it } from 'vitest';
import { categoriesFor as lookup, typeIcon as iconOf } from '../src/lib/type-registry';
import { CATEGORIES, OBJECT_TYPES, type Category } from '../src/lib/types';
import en from '../src/i18n/en';
import de from '../src/i18n/de';

// Built-in types need no own types to resolve; these tests read the built-in table alone.
const categoriesFor = (ty: string, current?: Category) => lookup(ty, [], current);
const typeIcon = (ty: string) => iconOf(ty, []);

describe('object types', () => {
  it('offers a bike no fuel and a body no inspection', () => {
    expect(categoriesFor('bike')).not.toContain('fuel');
    expect(categoriesFor('body')).toEqual(['weight', 'symptom', 'treatment', 'appointment', 'medication', 'other']);
    expect(categoriesFor('car')).toContain('fuel');
  });

  // Re-typing an object must not silently re-file its history: the select has to keep offering
  // whatever the entry already says, or saving an untouched form would change its category.
  it('always includes the entry\'s current category', () => {
    expect(categoriesFor('body', 'fuel')).toContain('fuel');
    expect(categoriesFor('bike', 'symptom')).toContain('symptom');
    expect(categoriesFor('car', 'fuel').filter((c) => c === 'fuel')).toHaveLength(1);
  });

  // `other` is defined as [...CATEGORIES], so scanning it made this assertion true by
  // construction: a bogus member added to CATEGORIES left it green. Excluding it means the
  // per-type tables are what gets tested -- a category no real type offers is unreachable from
  // the UI, and this is the only thing that says so.
  //
  // `trip` is excluded too, for a different reason: it is deliberately absent from every
  // built-in type's own category list (the spec: "The `trip` category is accepted for every
  // object type with a distance counter, independent of the type's category list"). It is
  // reachable by `counter_unit` (km/mi) instead, not by type -- that offering rule lives in the
  // trip-log UI, not in `TABLE` here, so this static, per-type scan cannot see it either way.
  it('every type has an icon and every category is reachable from a real type', () => {
    for (const t of OBJECT_TYPES) expect(typeIcon(t)).toBeTruthy();
    const specific = OBJECT_TYPES.filter((t) => t !== 'other');
    const reachable = new Set(specific.flatMap((t) => categoriesFor(t)));
    // `usage`, like `trip`, is capability-derived: a resource kind/unit adds it.
    for (const c of CATEGORIES.filter((c) => c !== 'trip' && c !== 'usage')) {
      expect(reachable, `no type but 'other' offers ${c}`).toContain(c);
    }
  });

  // A key in one language and not the other ships a screen with a raw key on it.
  it('both languages name every type and every category', () => {
    for (const t of OBJECT_TYPES) {
      expect(en[`type.${t}`], `en type.${t}`).toBeTruthy();
      expect(de[`type.${t}`], `de type.${t}`).toBeTruthy();
    }
    for (const c of CATEGORIES) {
      expect(en[`cat.${c}`], `en cat.${c}`).toBeTruthy();
      expect(de[`cat.${c}`], `de cat.${c}`).toBeTruthy();
    }
  });
});
