import { describe, it, expect } from 'vitest';
import { money, fmtDate, counter, todayIso } from '../src/lib/format';

describe('format', () => {
  it('formats cents as currency', () => {
    expect(money(123456, 'EUR', 'en')).toBe('€1,234.56');
    expect(money(123456, 'EUR', 'de').replace(/\u00a0/g, ' ')).toBe('1.234,56 €');
    expect(money(null, 'EUR', 'en')).toBe('');
  });
  it('formats dates per locale', () => {
    expect(fmtDate('2024-03-05', 'en')).toBe('Mar 5, 2024');
    expect(fmtDate('2024-03-05', 'de')).toBe('05.03.2024');
    expect(fmtDate(null, 'de')).toBe('');
  });
  it('formats counters with unit', () => {
    expect(counter(104500, 'km', 'de')).toBe('104.500 km');
    expect(counter(12, 'h', 'en')).toBe('12 h');
    expect(counter(null, 'km', 'en')).toBe('');
    expect(counter(5, null, 'en')).toBe('5');
  });
  it('todayIso is YYYY-MM-DD', () => {
    expect(todayIso()).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});
