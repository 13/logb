import { describe, expect, it } from 'vitest';
import { flattenTree, periodLabel, sharePct, statsPath } from '../src/lib/stats';
import type { StatsObject } from '../src/lib/types';

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
