/** One bar of a `Chart`. `value` sizes it; `display` is what a reader is told; `label` names it in
 *  full ("Sep 26") and `tick` is the short text under it ("Sep"). */
export interface ChartBar { key: string; label: string; tick: string; value: number; display: string }

/** The bars worth drawing. A month with nothing in it -- no spend, no trip, or a usage the
 *  readings cannot measure (passed in as 0) -- is left out: twelve months with two fills are two
 *  bars, not ten empty tracks. Each bar keeps its own label, so a gap shows in the labels. */
export function visibleBars(items: ChartBar[]): ChartBar[] {
  return items.filter((b) => Number.isFinite(b.value) && b.value > 0);
}

/** A bar's height as a share of the chart, 0..1, against the tallest bar. */
export function barShare(value: number, max: number): number {
  return max > 0 ? Math.min(1, Math.max(0, value / max)) : 0;
}

/** Whether bar `i` of `n` gets its tick drawn. About six ticks fit a 300 px chart, so every
 *  `ceil(n / 6)`th bar is drawn, counted from the newest (last) so the current month is always
 *  labelled. The screen-reader table under the chart names every bar either way. */
export function tickShown(i: number, n: number): boolean {
  return (n - 1 - i) % Math.max(1, Math.ceil(n / 6)) === 0;
}
