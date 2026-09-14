import type { Activity } from './types';

/** One row of the timeline: an entry as it is, or a run of readings folded into one line. */
export type TimelineRow =
  | { kind: 'entry'; activity: Activity }
  | { kind: 'readings'; key: string; readings: Activity[] };

/**
 * Folds every run of two or more consecutive readings into one row.
 *
 * A monthly reading habit adds twelve lines a year, and between two services they say one thing:
 * the counter went from here to there. A single reading stays a line of its own -- folding one
 * item hides it for nothing. A pending (still queued) reading is never folded, because it has to
 * stay visible as not sent yet.
 *
 * The key is the run's newest id, so a run keeps its open/closed state across a reload for as
 * long as no newer reading joins it.
 */
export function foldReadings(items: Activity[]): TimelineRow[] {
  const rows: TimelineRow[] = [];
  let run: Activity[] = [];
  const flush = () => {
    if (run.length >= 2) rows.push({ kind: 'readings', key: `r${run[0].id}`, readings: run });
    else for (const activity of run) rows.push({ kind: 'entry', activity });
    run = [];
  };
  for (const activity of items) {
    if (activity.category === 'reading' && !activity.pending) {
      run.push(activity);
    } else {
      flush();
      rows.push({ kind: 'entry', activity });
    }
  }
  flush();
  return rows;
}

/** The lowest and highest reading in a folded run. */
export function readingSpan(readings: Activity[]): { from: number; to: number } | null {
  const values = readings.map((r) => r.counter_value).filter((v): v is number => v !== null);
  if (values.length === 0) return null;
  return { from: Math.min(...values), to: Math.max(...values) };
}
