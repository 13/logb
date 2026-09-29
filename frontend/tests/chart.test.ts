import { describe, expect, it } from 'vitest';
import { barShare, tickShown, visibleBars, type ChartBar } from '../src/lib/chart';

const bar = (key: string, value: number): ChartBar => ({ key, label: key, tick: key, value, display: String(value) });

describe('visibleBars', () => {
  it('drops empty and unmeasured months, keeping the order', () => {
    const bars = [bar('2026-01', 0), bar('2026-02', 1200), bar('2026-03', Number.NaN), bar('2026-04', 300)];
    expect(visibleBars(bars).map((b) => b.key)).toEqual(['2026-02', '2026-04']);
  });
  it('is empty when every month is', () => {
    expect(visibleBars([bar('a', 0), bar('b', 0)])).toEqual([]);
  });
});

describe('barShare', () => {
  it('is the value against the tallest bar, within 0..1', () => {
    expect(barShare(50, 200)).toBe(0.25);
    expect(barShare(200, 200)).toBe(1);
    expect(barShare(300, 200)).toBe(1);
    expect(barShare(5, 0)).toBe(0);
  });
});

describe('tickShown', () => {
  it('labels every bar up to six', () => {
    expect([0, 1, 2, 3, 4, 5].map((i) => tickShown(i, 6))).toEqual([true, true, true, true, true, true]);
  });
  it('labels every other bar past six, always the newest', () => {
    expect([9, 10, 11].map((i) => tickShown(i, 12))).toEqual([true, false, true]);
    expect(tickShown(0, 12)).toBe(false);
    expect(tickShown(0, 7)).toBe(true);
  });
  it('thins in step with the count: 40 bars get about six labels, newest included', () => {
    const shown = Array.from({ length: 40 }, (_, i) => i).filter((i) => tickShown(i, 40));
    expect(shown).toEqual([39, 32, 25, 18, 11, 4].reverse());
    expect(shown.length).toBeLessThanOrEqual(7);
  });
});
