import { describe, expect, it } from 'vitest';
import { fillLabel, insightsPath, monthLabel, sinceLabel } from '../src/lib/insights';

describe('insightsPath', () => {
  it('asks for the object alone by default', () => {
    expect(insightsPath(7, false)).toBe('/objects/7/insights');
  });
  it('asks for contents only when on', () => {
    expect(insightsPath(7, true)).toBe('/objects/7/insights?contents=true');
  });
});

describe('sinceLabel', () => {
  it('names the month and year the object has been owned since', () => {
    expect(sinceLabel('2024-05-01', 'en')).toBe('May 2024');
    expect(sinceLabel('2024-05-01', 'de')).toBe('Mai 2024');
  });
});

describe('monthLabel', () => {
  it('keeps the year, because a twelve-month window crosses one', () => {
    expect(monthLabel('2026-09', 'en')).toBe('Sep 26');
    expect(monthLabel('2025-09', 'en')).toBe('Sep 25');
    expect(monthLabel('2026-09', 'de')).toMatch(/26$/);
  });
});

describe('fillLabel', () => {
  it('shows day and month', () => {
    expect(fillLabel('2026-01-10', 'en')).toBe('Jan 10');
    expect(fillLabel('2026-01-10', 'de')).toMatch(/^10\. Jan/);
  });
});
