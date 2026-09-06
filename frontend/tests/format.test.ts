import { describe, it, expect } from 'vitest';
import { money, fmtDate, counter, todayIso, perCounter, quantity, parseQuantity } from '../src/lib/format';

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
    // 47_777 milli-cents/unit = 47.777 cents/unit = €0.47777/unit -> €0.48, NOT €47.78.
    // (cost_per_counter_milli is cents-per-unit scaled by 1000; /1000 gets cents, /100
    // more gets currency units, i.e. /100_000 overall.)
    expect(perCounter(47_777, 'EUR', 'en')).toBe('€0.48');
  });
  it('renders a realistic per-kilometre fuel cost end to end', () => {
    // A car costing 420.00 EUR (42_000 cents) of fuel over an 7_000 km fuel-window span:
    // cost_per_counter_milli = 42_000 * 1000 / 7_000 = 6_000 milli-cents/km -> €0.06/km.
    expect(perCounter(6_000, 'EUR', 'en')).toBe('€0.06');
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

describe('parseQuantity', () => {
  it('normalises a comma to a dot, like parseMoney', () => {
    expect(parseQuantity('41,3')).toBe(41_300);
  });
  it('accepts a dot', () => {
    expect(parseQuantity('41.3')).toBe(41_300);
  });
  it('accepts an integer', () => {
    expect(parseQuantity('41')).toBe(41_000);
  });
  it('treats empty string as no value', () => {
    expect(parseQuantity('')).toBeNull();
  });
  it('rejects non-numeric input as NaN', () => {
    expect(parseQuantity('abc')).toBeNaN();
  });
  it('rounds x1000 scaling exactly, without floating-point drift', () => {
    // These two are the ones that bite: 1.005 * 1000 is 1004.9999999999999 in IEEE 754 and
    // 3.14159 * 1000 is 3141.5899999999997, so both truncate one short without the rounding.
    expect(parseQuantity('1.005')).toBe(1005);
    expect(parseQuantity('3.14159')).toBe(3142);
    expect(parseQuantity('41.3')).toBe(41_300);
  });
});
