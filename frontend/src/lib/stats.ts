import type { Stats, StatsObject } from './types';
import { dateTimeFormat } from './intl-cache';

/** The `by_category` bucket for objects' purchase prices -- not an activity category. */
export const PURCHASE_PRICE = 'purchase_price';

/** The request for a selection. Defaults are left out, so the common case is a plain `/stats`. */
export function statsPath(year: string | null, purchases: boolean): string {
  const q = new URLSearchParams();
  if (year) q.set('year', year);
  if (purchases) q.set('purchases', 'true');
  const s = q.toString();
  return s ? `/stats?${s}` : '/stats';
}

/** `2026` stays `2026`; `2026-03` becomes the reader's short month name. The year is already
 *  on screen in the picker, so repeating it on twelve bars is noise. */
export function periodLabel(bucket: string, locale: string): string {
  if (bucket.length === 4) return bucket;
  const [y, m] = bucket.split('-').map(Number);
  return dateTimeFormat(locale, { month: 'short' }).format(new Date(Date.UTC(y, m - 1, 15)));
}

export function sharePct(part: number, total: number): number {
  return total > 0 ? Math.round((part / total) * 100) : 0;
}

export interface TreeRow { node: StatsObject; depth: number; hasChildren: boolean; expanded: boolean }

/** The rows on screen: roots, plus the children of every expanded node whose ancestors are all
 *  expanded too. Collapsing a parent hides its whole subtree without forgetting what was open. */
export function flattenTree(nodes: StatsObject[], expanded: ReadonlySet<number>, depth = 0): TreeRow[] {
  return nodes.flatMap((node) => {
    const open = expanded.has(node.id);
    const row: TreeRow = { node, depth, hasChildren: node.children.length > 0, expanded: open };
    return open ? [row, ...flattenTree(node.children, expanded, depth + 1)] : [row];
  });
}

/** A bar's full name: "Sep 2026". A year bucket stays as it is. */
export function monthLabel(bucket: string, locale: string): string {
  if (bucket.length === 4) return bucket;
  const [y, m] = bucket.split('-').map(Number);
  return dateTimeFormat(locale, { month: 'short', year: 'numeric' }).format(new Date(Date.UTC(y, m - 1, 15)));
}

export interface YearSummary {
  year: string;
  spent: number;
  /** What `spent` is compared with: the year before, over the same months (`through` = the last
   *  month counted) while `year` is still running, or all of it once `year` is over. */
  previous: { year: string; cents: number; through: number | null };
  /** Whole percent, or null when the year before has nothing to compare with. */
  changePct: number | null;
  /** The root object with the most spend, its contents included. */
  top: { id: number; name: string; cents: number } | null;
}

/**
 * The statistics page's three cards, from two `/stats?year=` answers. The running year is
 * compared with the same months of the year before -- September against a whole previous year
 * would always read as a drop.
 */
export function yearSummary(focus: Stats, before: Stats, year: string, today: string): YearSummary {
  const through = today.slice(0, 4) === year ? Number(today.slice(5, 7)) : null;
  const cents = through === null
    ? before.total_cents
    : before.over_time.filter((a) => Number(a.bucket.slice(5, 7)) <= through).reduce((sum, a) => sum + a.cost_cents, 0);
  const spent = focus.total_cents;
  const top = focus.by_object.reduce<StatsObject | null>((best, o) => (o.cost_cents > (best?.cost_cents ?? 0) ? o : best), null);
  return {
    year,
    spent,
    previous: { year: String(Number(year) - 1).padStart(4, '0'), cents, through },
    changePct: cents > 0 ? Math.round(((spent - cents) / cents) * 100) : null,
    top: top && { id: top.id, name: top.name, cents: top.cost_cents },
  };
}
