import { describe, it, expect } from 'vitest';
import { addMonthsIso, readingActivity, readingWarning } from '../src/lib/reading';

const ctx = { lastCounter: 50_000, lastDate: '2026-09-01', ratePerDayMilli: 40_000 };

describe('reading form checks', () => {
  it('says nothing for an ordinary reading', () => {
    expect(readingWarning(50_400, '2026-09-11', ctx)).toBeNull();
    expect(readingWarning(50_000, '2026-09-11', ctx)).toBeNull();
  });

  it('questions a reading lower than the last one', () => {
    expect(readingWarning(49_999, '2026-09-11', ctx)).toBe('lower');
  });

  it('questions a jump far beyond the usual rate, which is usually an extra digit', () => {
    // 10 days at 40 km/day is 400 km; 5x that is 2_000 km.
    expect(readingWarning(52_001, '2026-09-11', ctx)).toBe('implausible');
    expect(readingWarning(51_999, '2026-09-11', ctx)).toBeNull();
    expect(readingWarning(500_000, '2026-09-11', ctx)).toBe('implausible');
  });

  it('never questions a short absolute jump or a reading without history', () => {
    expect(readingWarning(50_300, '2026-09-01', ctx)).toBeNull();
    expect(readingWarning(900_000, '2026-09-11', { lastCounter: null, lastDate: null, ratePerDayMilli: null })).toBeNull();
    expect(readingWarning(900_000, '2026-09-11', { ...ctx, ratePerDayMilli: null })).toBeNull();
  });

  it('saves as a reading entry carrying only the value', () => {
    expect(readingActivity(51_000, '2026-09-11', 'Counter reading')).toEqual({
      date: '2026-09-11', category: 'reading', title: 'Counter reading', notes: '',
      counter_value: 51_000, cost_cents: null, quantity_milli: null,
    });
  });

  it('adds calendar months, clamping to the end of a short month', () => {
    expect(addMonthsIso('2026-09-13', 1)).toBe('2026-10-13');
    expect(addMonthsIso('2026-01-31', 1)).toBe('2026-02-28');
    expect(addMonthsIso('2026-12-15', 1)).toBe('2027-01-15');
  });
});
