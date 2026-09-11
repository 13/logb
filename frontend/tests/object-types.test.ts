import { describe, expect, it } from 'vitest';
import { OBJECT_TYPES, categoriesFor, typeIcon } from '../src/lib/object-types';
import { CATEGORIES } from '../src/lib/types';
import en from '../src/i18n/en';
import de from '../src/i18n/de';

describe('object types', () => {
  it('offers a bike no fuel and a body no inspection', () => {
    expect(categoriesFor('bike')).not.toContain('fuel');
    expect(categoriesFor('body')).toEqual(['symptom', 'treatment', 'appointment', 'medication', 'other']);
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
  it('every type has an icon and every category is reachable from a real type', () => {
    for (const t of OBJECT_TYPES) expect(typeIcon(t)).toBeTruthy();
    const specific = OBJECT_TYPES.filter((t) => t !== 'other');
    const reachable = new Set(specific.flatMap((t) => categoriesFor(t)));
    for (const c of CATEGORIES) expect(reachable, `no type but 'other' offers ${c}`).toContain(c);
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
