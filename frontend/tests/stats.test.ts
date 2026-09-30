import { describe, expect, it } from 'vitest';
import { flattenTree, monthLabel, periodLabel, sharePct, statsPath, yearSummary } from '../src/lib/stats';
import type { Stats, StatsObject } from '../src/lib/types';

const node = (id: number, cost_cents: number, children: StatsObject[] = []): StatsObject =>
  ({ id, name: `o${id}`, type: 'other', archived: false, cost_cents, children });

describe('statsPath', () => {
  it('sends nothing it does not need', () => {
    expect(statsPath(null, false)).toBe('/stats');
  });
  it('sends the year and the toggle', () => {
    expect(statsPath('2026', true)).toBe('/stats?year=2026&purchases=true');
    expect(statsPath(null, true)).toBe('/stats?purchases=true');
  });
});

describe('periodLabel', () => {
  it('shows a year as itself', () => {
    expect(periodLabel('2026', 'en')).toBe('2026');
  });
  it('shows a month as its short name, in the reader\'s language', () => {
    expect(periodLabel('2026-03', 'en')).toBe('Mar');
    expect(periodLabel('2026-03', 'de')).toMatch(/^Mär/);
  });
});

describe('sharePct', () => {
  it('rounds to a whole percent', () => {
    expect(sharePct(1, 3)).toBe(33);
  });
  it('is 0 of nothing rather than NaN', () => {
    expect(sharePct(0, 0)).toBe(0);
  });
});

describe('flattenTree', () => {
  const tree = [node(1, 300, [node(2, 200, [node(3, 50)])]), node(4, 100)];

  it('shows only roots until something is expanded', () => {
    const rows = flattenTree(tree, new Set());
    expect(rows.map((r) => [r.node.id, r.depth, r.hasChildren, r.expanded])).toEqual([[1, 0, true, false], [4, 0, false, false]]);
  });

  it('shows the children of an expanded node, one level at a time', () => {
    expect(flattenTree(tree, new Set([1])).map((r) => [r.node.id, r.depth])).toEqual([[1, 0], [2, 1], [4, 0]]);
    expect(flattenTree(tree, new Set([1, 2])).map((r) => [r.node.id, r.depth])).toEqual([[1, 0], [2, 1], [3, 2], [4, 0]]);
  });

  it('hides a grandchild when its grandparent is collapsed, even if its parent is marked expanded', () => {
    expect(flattenTree(tree, new Set([2])).map((r) => r.node.id)).toEqual([1, 4]);
  });
});

describe('monthLabel', () => {
  it('names a month with its year, and leaves a year alone', () => {
    expect(monthLabel('2026-03', 'en')).toBe('Mar 2026');
    expect(monthLabel('2026', 'en')).toBe('2026');
  });
});

describe('yearSummary', () => {
  const stats = (total: number, months: Array<[string, number]>, roots: Array<[number, string, number]> = []): Stats => ({
    total_cents: total,
    years: [],
    over_time: months.map(([bucket, cost_cents]) => ({ bucket, cost_cents })),
    by_object: roots.map(([id, name, cost_cents]) => ({ id, name, type: 'car', archived: false, cost_cents, children: [] })),
    by_type: [],
    by_category: [],
  });

  it('compares a past year with the whole year before it', () => {
    const s = yearSummary(stats(75_000, []), stats(100_000, [['2024-03', 40_000], ['2024-11', 60_000]]), '2025', '2026-09-15');
    expect(s.previous).toEqual({ year: '2024', cents: 100_000, through: null });
    expect(s.changePct).toBe(-25);
  });

  it('compares the running year with the same months of the year before', () => {
    const before = stats(1_000_000, [['2025-03', 100_000], ['2025-09', 50_000], ['2025-11', 850_000]]);
    const s = yearSummary(stats(300_000, []), before, '2026', '2026-09-15');
    expect(s.previous).toEqual({ year: '2025', cents: 150_000, through: 9 });
    expect(s.changePct).toBe(100);
  });

  it('gives no percentage when the year before spent nothing', () => {
    expect(yearSummary(stats(5_000, []), stats(0, []), '2026', '2026-01-02').changePct).toBeNull();
  });

  it('names the object that cost most, counting what is inside it', () => {
    const s = yearSummary(stats(900, [], [[1, 'House', 600], [2, 'Car', 300]]), stats(0, []), '2026', '2026-05-01');
    expect(s.top).toEqual({ id: 1, name: 'House', cents: 600 });
  });

  it('has no top object in a year without spend', () => {
    expect(yearSummary(stats(0, []), stats(0, []), '2026', '2026-05-01').top).toBeNull();
  });
});
