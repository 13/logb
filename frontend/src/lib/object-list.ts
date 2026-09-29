import type { MemObject, ObjectType } from './types';
import { foldTag } from './tags';
import { collator } from './intl-cache';

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

/** Joins an object's searchable fields. No query contains it (a typed query is trimmed text), so a
 *  match can never straddle two fields -- the same answer as testing each field on its own. */
const SEP = '\u0000';

/** Each object's folded name, description and tags, worked out once per object rather than once
 *  per object per keystroke. Keyed by the object itself: a load replaces the rows, and with them
 *  the entries, so an edited object is never searched by its old text. The type label is not in
 *  here -- it depends on the language and the own types, and is folded per render instead. */
const searchText = new WeakMap<MemObject, string>();
function textOf(o: MemObject): string {
  let text = searchText.get(o);
  if (text === undefined) {
    text = [o.name, o.description, ...o.tags].map(fold).join(SEP);
    searchText.set(o, text);
  }
  return text;
}

/** A matcher for one query, folded once: what `visibleRows` runs over every object. */
function queryMatcher(query: string, typeLabel: (t: ObjectType) => string): (o: MemObject) => boolean {
  const q = fold(query.trim());
  if (!q) return () => true;
  const labels = new Map<ObjectType, boolean>();
  return (o) => {
    let label = labels.get(o.type);
    if (label === undefined) {
      label = fold(typeLabel(o.type)).includes(q);
      labels.set(o.type, label);
    }
    return label || textOf(o).includes(q);
  };
}

export function matchesQuery(o: MemObject, query: string, typeLabel: (t: ObjectType) => string): boolean {
  return queryMatcher(query, typeLabel)(o);
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
  // One collator for the whole sort (and every later one in this locale): `localeCompare` with
  // options builds one per comparison, n·log n times per keystroke.
  const compare = collator(locale, { sensitivity: 'base' }).compare;
  const byName = (a: MemObject, b: MemObject) => compare(a.name, b.name);
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

/** The dashboard's active list: objects still only in the outbox first, then the server's rows.
 *  Pending rows already in `rows` are dropped first, so merging into an earlier merge (an
 *  offline load keeps what is on screen) never shows a queued object twice. */
export function withPendingObjects(queued: MemObject[], rows: MemObject[]): MemObject[] {
  return [...queued, ...rows.filter((o) => !o.pending)];
}

/** How many objects the active tab lists with no query: the top-level ones, counting an object
 *  whose parent is not active as top-level (see `visibleRows`). Without sorting anything. */
export function topLevelCount(active: MemObject[]): number {
  const ids = new Set(active.map((o) => o.id));
  let n = 0;
  for (const o of active) if (o.parent_id === null || !ids.has(o.parent_id)) n++;
  return n;
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
  const matches = queryMatcher(query, typeLabel);
  const shown = pool.filter((o) => {
    if (searching) {
      return matches(o) && (wanted === null || o.tags.some((x) => foldTag(x) === wanted));
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

/** What the listed objects have cost so far, for the dashboard's subtitle. Each object's own
 *  total counts once -- a child's costs are its own, not folded into its parent's -- and an
 *  object still waiting in the outbox has no server total yet. */
export function activeSpend(objects: MemObject[]): number {
  let sum = 0;
  for (const o of objects) if (!o.pending) sum += o.stats.total_cost_cents;
  return sum;
}
