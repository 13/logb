import { describe, expect, it } from 'vitest';
import { foldReadings, readingSpan } from '../src/lib/timeline-fold';
import type { Activity, Category } from '../src/lib/types';

function a(id: number, category: Category, counter: number | null = null, pending = false): Activity {
  return {
    id, object_id: 1, date: '2026-09-01', category, title: `t${id}`, notes: '', counter_value: counter,
    cost_cents: null, quantity_milli: null, created_at: '', updated_at: '', attachments: [], pending, tags: [],
    start_counter: null, from_place: null, to_place: null, duration_minutes: null, battery_used_pct: null,
  };
}

describe('foldReadings', () => {
  it('folds a run of readings into one row and leaves everything else alone', () => {
    const rows = foldReadings([a(5, 'reading', 1300), a(4, 'reading', 1200), a(3, 'reading', 1100), a(2, 'repair'), a(1, 'reading', 1000)]);
    expect(rows.map((r) => r.kind)).toEqual(['readings', 'entry', 'entry']);
    const run = rows[0];
    if (run.kind !== 'readings') throw new Error('expected a run');
    expect(run.readings.map((r) => r.id)).toEqual([5, 4, 3]);
    expect(run.key).toBe('r5');
  });

  it('does not fold a single reading, which would hide it for nothing', () => {
    expect(foldReadings([a(2, 'reading', 10), a(1, 'fuel', 5)]).map((r) => r.kind)).toEqual(['entry', 'entry']);
  });

  it('never folds a reading still waiting to be sent', () => {
    const rows = foldReadings([a(-1, 'reading', 30, true), a(2, 'reading', 20), a(1, 'reading', 10)]);
    expect(rows.map((r) => r.kind)).toEqual(['entry', 'readings']);
  });

  // A trip has its own counter fields (start/end), and reads like a real entry, not a bare
  // number -- folding it into a reading run would hide exactly the trip detail the timeline
  // exists to show. The fold already keys purely on `category === 'reading'` (see
  // `foldReadings` above), so a trip breaks a run the same way any other category does; this
  // just pins that down for trips specifically, since a regression here would silently start
  // swallowing trips into reading runs.
  it('a trip between two readings breaks the run instead of joining it', () => {
    const rows = foldReadings([a(3, 'reading', 30), a(2, 'trip', 20), a(1, 'reading', 10)]);
    expect(rows.map((r) => r.kind)).toEqual(['entry', 'entry', 'entry']);
  });

  it('spans the lowest to the highest value in the run', () => {
    expect(readingSpan([a(2, 'reading', 1300), a(1, 'reading', 1100)])).toEqual({ from: 1100, to: 1300 });
    expect(readingSpan([a(1, 'reading', null)])).toBeNull();
  });
});
