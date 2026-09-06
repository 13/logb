import { describe, it, expect } from 'vitest';
import { money, fmtDate, counter, todayIso, perCounter, quantity } from '../src/lib/format';

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

describe('perCounter', () => {
  it('renders milli-cents per unit as money with two decimals', () => {
    expect(perCounter(47_777, 'EUR', 'en')).toBe('€47.78');
  });
  it('renders nothing when there is no value', () => {
    expect(perCounter(null, 'EUR', 'en')).toBe('');
  });
});

describe('quantity', () => {
  it('renders milli-units with one decimal and the unit', () => {
    expect(quantity(41_300, 'l', 'en')).toBe('41.3 l');
  });
  it('renders nothing when there is no value', () => {
    expect(quantity(null, 'l', 'en')).toBe('');
  });
});
