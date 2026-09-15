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
 *  settings, chevron and the like -- are left out because on a card they would read as controls,
 *  and `box` because it draws almost the same cube as `object`: two identical picker choices. */
export const CUSTOM_TYPE_ICONS: IconName[] = [
  'document', 'camera', 'car', 'e-bike', 'bike', 'motorcycle', 'home', 'appliance', 'tool', 'body',
  'object',
];

/**
 * The signed-in user's own types.
 *
 * Module scope, so the list outlives any one screen. The last list that loaded is kept per user
 * in `localStorage` (`logb.types.<userId>`) and read back before the network answers: an app
 * started without a connection still names and draws its types. The `GET /api/types` response
 * itself is also in the service worker's `logb-api` cache (NetworkFirst), which answers the
 * request when offline; the stored list is what shows before any answer at all.
 *
 * Cleared, with the object cache, whenever a session ends -- and when a session starts for
 * someone other than the caches' recorded owner (see `../stores/session.ts`), which also drops
 * every stored list (`clearStoredTypeLists`).
 */
export const customTypes: Writable<CustomType[]> = writable([]);

/** True once the list is known: a `GET /types` succeeded, or a stored list was read. Until then
 *  an unknown `custom:` key is merely not loaded yet, and must not be called unknown. */
export const typesLoaded: Writable<boolean> = writable(false);

/** Whose list `customTypes` holds, so a load for someone else never shows the previous user's. */
let owner: number | null = null;

const storageKey = (userId: number) => `logb.types.${userId}`;

function readStored(userId: number): CustomType[] | null {
  try {
    const raw = globalThis.localStorage?.getItem(storageKey(userId));
    const list: unknown = raw ? JSON.parse(raw) : null;
    return Array.isArray(list) ? (list as CustomType[]) : null;
  } catch {
    return null;
  }
}

function writeStored(userId: number, list: CustomType[]): void {
  try { globalThis.localStorage?.setItem(storageKey(userId), JSON.stringify(list)); } catch { /* full or blocked: the next load tries again */ }
}

/** Makes `userId` the list's owner: a different user's list is dropped at once, and the stored
 *  list for this one, if any, stands in until the network answers. */
function adopt(userId: number): void {
  if (owner === userId) return;
  owner = userId;
  const stored = readStored(userId);
  customTypes.set(stored ?? []);
  typesLoaded.set(stored !== null);
}

/** Loads `userId`'s types; with no argument, reloads the current owner's (a screen that just
 *  changed a type). Nothing to do when nobody owns the list yet: `App` loads it per user. */
export async function loadCustomTypes(userId: number | null = owner): Promise<void> {
  if (userId === null) return;
  adopt(userId);
  try {
    const list = await api<CustomType[]>('GET', '/types');
    // The session may have ended or changed hands while this was on the wire.
    if (owner !== userId) return;
    customTypes.set(list);
    typesLoaded.set(true);
    writeStored(userId, list);
  } catch {
    // Keep what is already here. With no connection and nothing stored, objects of an own type
    // show a neutral placeholder until the next load succeeds -- better than an error on every
    // screen.
  }
}

export function clearCustomTypes(): void {
  if (owner !== null) {
    try { globalThis.localStorage?.removeItem(storageKey(owner)); } catch { /* nothing to clear */ }
  }
  owner = null;
  customTypes.set([]);
  typesLoaded.set(false);
}

/** Removes every user's stored list, not just the current owner's. After a reload this tab knows
 *  no owner, so when the device changes hands that is the only way the previous person's list
 *  leaves the disk. */
export function clearStoredTypeLists(): void {
  try {
    const ls = globalThis.localStorage;
    if (!ls) return;
    const keys: string[] = [];
    for (let i = 0; i < ls.length; i++) {
      const k = ls.key(i);
      if (k?.startsWith('logb.types.')) keys.push(k);
    }
    for (const k of keys) ls.removeItem(k);
  } catch { /* blocked: nothing could have been stored either */ }
}

function isBuiltin(key: string): key is BuiltinType {
  return (OBJECT_TYPES as readonly string[]).includes(key);
}

function find(key: string, custom: CustomType[]): CustomType | undefined {
  return key.startsWith('custom:') ? custom.find((c) => c.key === key) : undefined;
}

/** Built-in: its translation. Own type: its name. An own type missing from the list still needs
 *  words on the card: a neutral placeholder while the list is still `loaded === false`, and
 *  "Unknown type" after -- deleted elsewhere or not synced here yet, this device cannot tell. */
export function typeLabel(key: string, custom: CustomType[], t: (k: string) => string, loaded: boolean): string {
  if (isBuiltin(key)) return t(`type.${key}`);
  return find(key, custom)?.name ?? (loaded ? t('types.unknown') : t('types.loading'));
}

export function typeIcon(key: string, custom: CustomType[]): IconName {
  if (isBuiltin(key)) return TABLE[key].icon;
  return find(key, custom)?.icon ?? 'object';
}

/**
 * What the category select offers for this type -- plus `current`, always, plus `trip` when
 * `counterUnit` is a distance unit.
 *
 * Filtering is presentation only. An entry logged before its object was re-typed keeps its own
 * category in the list, so opening and saving an untouched form cannot re-file it. An unknown
 * type offers everything, for the same reason.
 *
 * `trip` is offered by `counterUnit`, not by type: the spec ("The `trip` category is accepted
 * for every object type with a distance counter, independent of the type's category list")
 * means no built-in type's own list in `TABLE` names it (see the carve-out in
 * `builtin-types.test.ts`) -- it is added here instead, exactly when the object could actually
 * hold one (`km`/`mi`), or when `current` already is one (re-typing away from km/mi must not
 * hide an existing trip's own category, the same reason `current` is always kept below).
 */
export function categoriesFor(key: string, custom: CustomType[], current?: Category, counterUnit?: CounterUnit): Category[] {
  const list = isBuiltin(key) ? TABLE[key].categories : find(key, custom)?.categories ?? [...CATEGORIES];
  const offersTrip = counterUnit === 'km' || counterUnit === 'mi' || current === 'trip';
  // Annotated explicitly: with no contextual type here, a bare `'trip'` in `[...list, 'trip']`
  // widens to plain `string`, and the `Category[] | string[]` that leaves `withTrip` with then
  // fails `.includes(current)` below (a `Category`) on the `string[]` branch.
  const withTrip: Category[] = offersTrip && !list.includes('trip') ? [...list, 'trip'] : list;
  return current && !withTrip.includes(current) ? [...withTrip, current] : withTrip;
}

/** The unit a new object of this type starts with. Built-in types leave it to the user. */
export function defaultUnit(key: string, custom: CustomType[]): CounterUnit {
  return find(key, custom)?.counter_unit ?? null;
}
