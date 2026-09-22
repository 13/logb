import { hashToNegativeId } from './activity-form';
import { foldTag } from './tags';
import type { QueuedOp } from './outbox';
import type { Activity, ActivityInput, Category, MemObject } from './types';

/** The `?tag=` of a search string, trimmed; blank or absent is `null`. */
export function tagParam(search: string): string | null {
  return new URLSearchParams(search).get('tag')?.trim() || null;
}

/** Whether a trip can be logged here at all -- the same condition `categoriesFor`'s
 *  `counterUnit` argument checks, so the button and the form's category list never disagree. */
export function offersTrip(o: MemObject | null): boolean {
  return o?.counter_unit === 'km' || o?.counter_unit === 'mi';
}

export function offersEnergy(o: MemObject | null): boolean {
  return (o?.resource_unit ?? o?.fuel_unit) != null;
}

export function resourceCategory(o: MemObject | null): 'usage' | 'fuel' {
  return o?.resource_kind ? 'usage' : 'fuel';
}

/** The same fold the server's `title` filter uses (`fold_title` in `api/activities`). */
export function foldTitle(title: string): string {
  return title.trim().toLowerCase();
}

/** A queued 'activity.create' has no server row yet, so it renders straight from what the form
 *  queued. `pending: true` tells `Timeline` to dim it and refuse navigation into its fake id. */
export function pendingToActivity(op: QueuedOp, oid: number): Activity {
  const b = op.body as Partial<ActivityInput>;
  const now = new Date().toISOString();
  return {
    id: hashToNegativeId(op.id), object_id: oid,
    date: typeof b.date === 'string' ? b.date : now.slice(0, 10),
    category: (b.category as Category) ?? 'other',
    title: typeof b.title === 'string' ? b.title : '',
    notes: typeof b.notes === 'string' ? b.notes : '',
    weight_grams: typeof b.weight_grams === 'number' ? b.weight_grams : null,
    counter_value: typeof b.counter_value === 'number' ? b.counter_value : null,
    cost_cents: typeof b.cost_cents === 'number' ? b.cost_cents : null,
    quantity_milli: typeof b.quantity_milli === 'number' ? b.quantity_milli : null,
    start_counter: typeof b.start_counter === 'number' ? b.start_counter : null,
    from_place: typeof b.from_place === 'string' ? b.from_place : null,
    to_place: typeof b.to_place === 'string' ? b.to_place : null,
    duration_minutes: typeof b.duration_minutes === 'number' ? b.duration_minutes : null,
    battery_used_pct: typeof b.battery_used_pct === 'number' ? b.battery_used_pct : null,
    charged_full: typeof b.charged_full === 'number' ? b.charged_full : 0,
    fuel_level_pct: typeof b.fuel_level_pct === 'number' ? b.fuel_level_pct : null,
    meter_reading_milli: typeof b.meter_reading_milli === 'number' ? b.meter_reading_milli : null,
    period_start: typeof b.period_start === 'string' ? b.period_start : null,
    period_end: typeof b.period_end === 'string' ? b.period_end : null,
    estimated: typeof b.estimated === 'number' ? b.estimated : 0,
    meter_reset: typeof b.meter_reset === 'number' ? b.meter_reset : 0,
    created_at: now, updated_at: now, attachments: [],
    pending: true, tags: Array.isArray(b.tags) ? b.tags : [],
  };
}

/** The server filters the loaded page by category, tag and title; a queued entry has not
 *  reached it, so it is filtered here the same way. */
export function filterPendingOps(ops: QueuedOp[], f: { category: string | null; tagFilter: string | null; titleFilter: string | null }): QueuedOp[] {
  const wantedTag = f.tagFilter === null ? null : foldTag(f.tagFilter);
  const wantedTitle = f.titleFilter === null ? null : foldTitle(f.titleFilter);
  return ops
    .filter((o) => !f.category || o.body.category === f.category)
    .filter((o) => wantedTag === null || (Array.isArray(o.body.tags) && (o.body.tags as string[]).some((x) => foldTag(x) === wantedTag)))
    .filter((o) => wantedTitle === null || (typeof o.body.title === 'string' && foldTitle(o.body.title) === wantedTitle));
}

/** The address for the current tab: the default tab is left out and `?tag=` is dropped, since
 *  the tag filter is session state once read. */
export function nextUrl(href: string, tab: string): string {
  const url = new URL(href);
  if (tab === 'timeline') url.searchParams.delete('tab'); else url.searchParams.set('tab', tab);
  url.searchParams.delete('tag');
  return url.pathname + url.search;
}
