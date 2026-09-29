import { describe, expect, it } from 'vitest';
import { lateness } from '../src/lib/lateness';

const t = (k: string, v?: Record<string, string | number>) => (v ? `${k}:${JSON.stringify(v)}` : k);

describe('lateness', () => {
  it('says nothing about a reminder with no date', () => {
    expect(lateness({ days_until: null }, t)).toBe('');
  });
  it('today, one day, many days', () => {
    expect(lateness({ days_until: 0 }, t)).toBe('dash.due-today');
    expect(lateness({ days_until: -1 }, t)).toBe('dash.overdue-day');
    expect(lateness({ days_until: -12 }, t)).toBe('dash.overdue-days:{"n":12}');
  });
  it('says nothing about one not due yet', () => {
    expect(lateness({ days_until: 3 }, t)).toBe('');
  });
});
