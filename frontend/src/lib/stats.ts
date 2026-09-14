import type { StatsObject } from './types';

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
  return new Intl.DateTimeFormat(locale, { month: 'short' }).format(new Date(Date.UTC(y, m - 1, 15)));
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
