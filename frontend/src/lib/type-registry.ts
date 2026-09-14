import { writable, type Writable } from 'svelte/store';
import { api } from './api';
import { CATEGORIES, OBJECT_TYPES, type BuiltinType, type Category, type CounterUnit, type CustomType } from './types';
import type { IconName } from './Icon.svelte';

/** What each built-in type is, in one place: the icon a row shows, and what its entries can be. */
const TABLE: Record<BuiltinType, { icon: IconName; categories: Category[] }> = {
  car:        { icon: 'car',        categories: ['maintenance', 'repair', 'inspection', 'fuel', 'reading', 'modification', 'purchase', 'other'] },
  e_bike:     { icon: 'e-bike',     categories: ['maintenance', 'repair', 'inspection', 'fuel', 'reading', 'modification', 'purchase', 'other'] },
  bike:       { icon: 'bike',       categories: ['maintenance', 'repair', 'inspection', 'reading', 'modification', 'purchase', 'other'] },
  motorcycle: { icon: 'motorcycle', categories: ['maintenance', 'repair', 'inspection', 'fuel', 'reading', 'modification', 'purchase', 'other'] },
  home:       { icon: 'home',       categories: ['maintenance', 'repair', 'inspection', 'modification', 'purchase', 'other'] },
  appliance:  { icon: 'appliance',  categories: ['maintenance', 'repair', 'inspection', 'reading', 'modification', 'purchase', 'other'] },
  tool:       { icon: 'tool',       categories: ['maintenance', 'repair', 'inspection', 'reading', 'modification', 'purchase', 'other'] },
  body:       { icon: 'body',       categories: ['symptom', 'treatment', 'appointment', 'medication', 'other'] },
  other:      { icon: 'object',     categories: [...CATEGORIES] },
};

/** The icons an own type may have: the same list, in the same order, as `CUSTOM_TYPE_ICONS` in
 *  `src/domain/custom_type.rs` (tests/icons.test.ts compares the two). UI-only icons -- back,
 *  settings, chevron and the like -- are left out because on a card they would read as controls. */
export const CUSTOM_TYPE_ICONS: IconName[] = [
  'document', 'camera', 'car', 'e-bike', 'bike', 'motorcycle', 'home', 'appliance', 'tool', 'body',
  'object', 'box',
];

/**
 * The signed-in user's own types.
 *
 * Cached the way objects are: module scope, so it outlives any one screen, and the `GET` goes
 * through the service worker's NetworkFirst `logb-api` cache, so a device that starts offline
 * still gets the last list it saw. Cleared with the object cache whenever a session ends.
 */
export const customTypes: Writable<CustomType[]> = writable([]);

export async function loadCustomTypes(): Promise<void> {
  try {
    customTypes.set(await api<CustomType[]>('GET', '/types'));
  } catch {
    // Keep what is already here. With no connection and nothing cached, objects of an own type
    // show as "Deleted type" until the next load succeeds -- better than an error on every screen.
  }
}

export function clearCustomTypes(): void {
  customTypes.set([]);
}

function isBuiltin(key: string): key is BuiltinType {
  return (OBJECT_TYPES as readonly string[]).includes(key);
}

function find(key: string, custom: CustomType[]): CustomType | undefined {
  return key.startsWith('custom:') ? custom.find((c) => c.key === key) : undefined;
}

/** Built-in: its translation. Own type: its name. An own type that is gone (deleted elsewhere,
 *  or not synced to this device yet) still needs words on the card, so it says so. */
export function typeLabel(key: string, custom: CustomType[], t: (k: string) => string): string {
  if (isBuiltin(key)) return t(`type.${key}`);
  return find(key, custom)?.name ?? t('types.deleted');
}

export function typeIcon(key: string, custom: CustomType[]): IconName {
  if (isBuiltin(key)) return TABLE[key].icon;
  return find(key, custom)?.icon ?? 'object';
}

/**
 * What the category select offers for this type -- plus `current`, always.
 *
 * Filtering is presentation only. An entry logged before its object was re-typed keeps its own
 * category in the list, so opening and saving an untouched form cannot re-file it. An unknown
 * type offers everything, for the same reason.
 */
export function categoriesFor(key: string, custom: CustomType[], current?: Category): Category[] {
  const list = isBuiltin(key) ? TABLE[key].categories : find(key, custom)?.categories ?? [...CATEGORIES];
  return current && !list.includes(current) ? [...list, current] : list;
}

/** The unit a new object of this type starts with. Built-in types leave it to the user. */
export function defaultUnit(key: string, custom: CustomType[]): CounterUnit {
  return find(key, custom)?.counter_unit ?? null;
}
