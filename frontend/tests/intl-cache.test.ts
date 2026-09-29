import { describe, expect, it } from 'vitest';
import { collator, dateTimeFormat, numberFormat, relativeTimeFormat } from '../src/lib/intl-cache';
import { money } from '../src/lib/format';

describe('cached Intl formatters', () => {
  it('hands out one formatter per locale and options', () => {
    const a = numberFormat('de', { maximumFractionDigits: 1 });
    expect(numberFormat('de', { maximumFractionDigits: 1 })).toBe(a);
    expect(numberFormat('en', { maximumFractionDigits: 1 })).not.toBe(a);
    expect(numberFormat('de', { maximumFractionDigits: 2 })).not.toBe(a);
    expect(numberFormat('de')).not.toBe(a);
    expect(numberFormat('de')).toBe(numberFormat('de'));
  });

  it('formats exactly as a fresh formatter would', () => {
    expect(numberFormat('de', { maximumFractionDigits: 1 }).format(1234.56)).toBe(
      new Intl.NumberFormat('de', { maximumFractionDigits: 1 }).format(1234.56),
    );
    const opts: Intl.DateTimeFormatOptions = { month: 'short', year: 'numeric', timeZone: 'UTC' };
    const day = new Date(Date.UTC(2026, 8, 15, 12));
    expect(dateTimeFormat('en', opts).format(day)).toBe(new Intl.DateTimeFormat('en', opts).format(day));
    expect(relativeTimeFormat('en', { numeric: 'auto' }).format(-1, 'day')).toBe('yesterday');
    expect(['b', 'Ärger', 'c'].sort(collator('de', { sensitivity: 'base' }).compare)).toEqual(['Ärger', 'b', 'c']);
  });

  it('keeps throwing for options a formatter refuses, and caches nothing for them', () => {
    expect(() => money(100, 'NOT-A-CURRENCY', 'en')).toThrow(RangeError);
    expect(() => money(100, 'NOT-A-CURRENCY', 'en')).toThrow(RangeError);
    expect(money(100, 'EUR', 'en')).toBe('€1.00');
  });
});
