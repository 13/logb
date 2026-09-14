import type { MemObject, ObjectType } from './types';
import { foldTag } from './tags';

export const SORT_KEYS = ['name', 'last-activity', 'changed', 'cost', 'counter'] as const;
export type SortKey = (typeof SORT_KEYS)[number];
export type ListTab = 'active' | 'archived';

export function parseSort(value: string | null | undefined): SortKey | null {
  return (SORT_KEYS as readonly string[]).includes(value ?? '') ? (value as SortKey) : null;
}

export function parseTab(value: string | null | undefined): ListTab {
  return value === 'archived' ? 'archived' : 'active';
}

/** Lower case with accents removed, so "fahrrader" finds "Fahrräder". */
function fold(s: string): string {
  // Not `toLocaleLowerCase`, which depends on the device's locale; see `foldTag`.
  return s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
}

export function matchesQuery(o: MemObject, query: string, typeLabel: (t: ObjectType) => string): boolean {
  const q = fold(query.trim());
  if (!q) return true;
  return [o.name, typeLabel(o.type), o.description, ...o.tags].some((field) => fold(field).includes(q));
}

/** The value each non-name sort orders by, highest or newest first. Null means "not known", and
 *  goes last: an object never used is not more recent than one used last year. A cost of 0 is
 *  treated as unknown for the same reason. ISO dates and RFC 3339 timestamps compare as text. */
const VALUE: Record<Exclude<SortKey, 'name'>, (o: MemObject) => string | number | null> = {
  'last-activity': (o) => o.stats.last_activity_date,
  changed: (o) => o.updated_at,
  cost: (o) => (o.stats.total_cost_cents > 0 ? o.stats.total_cost_cents : null),
  counter: (o) => o.stats.current_counter,
};

export function sortObjects(list: MemObject[], key: SortKey, locale: string): MemObject[] {
  const byName = (a: MemObject, b: MemObject) => a.name.localeCompare(b.name, locale, { sensitivity: 'base' });
  if (key === 'name') return [...list].sort(byName);
  const value = VALUE[key];
  return [...list].sort((a, b) => {
    const va = value(a);
    const vb = value(b);
    if (va === null || vb === null) return va === vb ? byName(a, b) : va === null ? 1 : -1;
    if (va !== vb) return va < vb ? 1 : -1;
    return byName(a, b);
  });
}

export interface ListRow { object: MemObject; parentName: string | null }

/** What the list shows for a tab, a query, a sort and a tag filter.
 *
 *  The active tab without a query shows top-level objects, as the dashboard always has -- a
 *  child is reached through its parent. An object whose parent is not active counts as
 *  top-level there, or a live child of an archived parent would appear nowhere. A query searches
 *  every depth, and the archived tab is flat; both name each object's parent so two "Filter"s in
 *  different rooms can be told apart. A tag filter is a search too: the boiler tagged "winter"
 *  lives inside the house, and a filter that only looked at top-level objects would hide it. */
export function visibleRows(
  active: MemObject[], archived: MemObject[], tab: ListTab, query: string, sort: SortKey,
  typeLabel: (t: ObjectType) => string, locale: string, tag: string | null = null,
): ListRow[] {
  const names = new Map<number, string>([...active, ...archived].map((o) => [o.id, o.name]));
  const activeIds = new Set(active.map((o) => o.id));
  const searching = query.trim() !== '' || tag !== null;
  const wanted = tag === null ? null : foldTag(tag);
  const pool = tab === 'archived' ? archived : active;
  const shown = pool.filter((o) => {
    if (searching) {
      return matchesQuery(o, query, typeLabel) && (wanted === null || o.tags.some((x) => foldTag(x) === wanted));
    }
    if (tab === 'archived') return true;
    return o.parent_id === null || !activeIds.has(o.parent_id);
  });
  const withParent = searching || tab === 'archived';
  return sortObjects(shown, sort, locale).map((object) => ({
    object,
    parentName: withParent && object.parent_id !== null ? names.get(object.parent_id) ?? null : null,
  }));
}
