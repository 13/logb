import { CATEGORIES, OBJECT_TYPES, type Category, type ObjectType } from './types';

/** What each type is, in one place: the icon a row shows, and what its entries can be. */
const TABLE: Record<ObjectType, { icon: string; categories: Category[] }> = {
  car:        { icon: 'car',        categories: ['maintenance', 'repair', 'inspection', 'fuel', 'modification', 'purchase', 'other'] },
  e_bike:     { icon: 'e-bike',     categories: ['maintenance', 'repair', 'inspection', 'fuel', 'modification', 'purchase', 'other'] },
  bike:       { icon: 'bike',       categories: ['maintenance', 'repair', 'inspection', 'modification', 'purchase', 'other'] },
  motorcycle: { icon: 'motorcycle', categories: ['maintenance', 'repair', 'inspection', 'fuel', 'modification', 'purchase', 'other'] },
  home:       { icon: 'home',       categories: ['maintenance', 'repair', 'inspection', 'modification', 'purchase', 'other'] },
  appliance:  { icon: 'appliance',  categories: ['maintenance', 'repair', 'inspection', 'modification', 'purchase', 'other'] },
  tool:       { icon: 'tool',       categories: ['maintenance', 'repair', 'inspection', 'modification', 'purchase', 'other'] },
  body:       { icon: 'body',       categories: ['symptom', 'treatment', 'appointment', 'medication', 'other'] },
  other:      { icon: 'object',     categories: [...CATEGORIES] },
};

export { OBJECT_TYPES };
export function typeIcon(t: ObjectType): string { return TABLE[t].icon; }

/**
 * What the category select offers for this type -- plus `current`, always.
 *
 * Filtering is presentation only. An entry logged before its object was re-typed keeps its own
 * category in the list, so opening and saving an untouched form cannot re-file it.
 */
export function categoriesFor(t: ObjectType, current?: Category): Category[] {
  const list = TABLE[t].categories;
  return current && !list.includes(current) ? [...list, current] : list;
}
