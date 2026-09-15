import { describe, it, expect } from 'vitest';
import {
  money, moneyWhole, fmtDate, parseDate, resolveDateFormat, datePlaceholder,
  counter, todayIso, perCounter, quantity, parseQuantity, lastActivityLabel,
} from '../src/lib/format';

describe('format', () => {
  it('formats cents as currency', () => {
    expect(money(123456, 'EUR', 'en')).toBe('€1,234.56');
    expect(money(123456, 'EUR', 'de').replace(/\u00a0/g, ' ')).toBe('1.234,56 €');
    expect(money(null, 'EUR', 'en')).toBe('');
  });
  it('formats cents as whole-unit currency, rounded', () => {
    expect(moneyWhole(179_128, 'EUR', 'en')).toBe('\u{20ac}1,791');
    expect(moneyWhole(150, 'EUR', 'en')).toBe('\u{20ac}2');
    expect(moneyWhole(null, 'EUR', 'en')).toBe('');
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

describe('fmtDate', () => {
  it('renders each format with zero-padded day and month', () => {
    expect(fmtDate('2026-09-05', 'dmy-dot')).toBe('05.09.2026');
    expect(fmtDate('2026-09-05', 'dmy-slash')).toBe('05/09/2026');
    expect(fmtDate('2026-09-05', 'mdy-slash')).toBe('09/05/2026');
    expect(fmtDate('2026-09-05', 'iso')).toBe('2026-09-05');
  });
  it('takes the date part of a timestamp and ignores empty input', () => {
    expect(fmtDate('2026-09-05T23:30:00Z', 'dmy-dot')).toBe('05.09.2026');
    expect(fmtDate(null, 'dmy-dot')).toBe('');
    expect(fmtDate('', 'iso')).toBe('');
  });
});

describe('parseDate', () => {
  it('reads the chosen pattern and lenient variants', () => {
    expect(parseDate('15.09.2026', 'dmy-dot')).toBe('2026-09-15');
    expect(parseDate('1.5.26', 'dmy-dot')).toBe('2026-05-01');
    expect(parseDate('1/5/2026', 'dmy-slash')).toBe('2026-05-01');
    expect(parseDate('5-1-2026', 'mdy-slash')).toBe('2026-05-01');
    expect(parseDate('2026-05-01', 'iso')).toBe('2026-05-01');
    expect(parseDate(' 15.09.2026 ', 'dmy-dot')).toBe('2026-09-15');
  });
  it('rejects impossible or malformed dates', () => {
    expect(parseDate('31.02.2026', 'dmy-dot')).toBeNull();
    expect(parseDate('13/13/2026', 'mdy-slash')).toBeNull();
    expect(parseDate('15.09', 'dmy-dot')).toBeNull();
    expect(parseDate('abc', 'iso')).toBeNull();
    expect(parseDate('', 'dmy-dot')).toBeNull();
  });
  it('also reads a comma or a single space as a separator', () => {
    expect(parseDate('15,09,2026', 'dmy-dot')).toBe('2026-09-15');
    expect(parseDate('15 09 2026', 'dmy-dot')).toBe('2026-09-15');
  });
  it('rejects a doubled separator rather than treating it as one', () => {
    expect(parseDate('15  09 2026', 'dmy-dot')).toBeNull();
  });
  // The iOS numeric keypad (what `inputmode="numeric"` offers there) has no `.`/`-`/`/` key at
  // all, so a date typed on it has no separators -- these read the digits in the chosen
  // format's own field order instead.
  it('reads separator-free digits in the format\'s own field order', () => {
    expect(parseDate('15092026', 'dmy-dot')).toBe('2026-09-15');
    expect(parseDate('15092026', 'dmy-slash')).toBe('2026-09-15');
    expect(parseDate('09152026', 'mdy-slash')).toBe('2026-09-15');
    expect(parseDate('20260915', 'iso')).toBe('2026-09-15');
  });
  it('reads 6 separator-free digits as a 2-digit year, except for iso', () => {
    expect(parseDate('150926', 'dmy-dot')).toBe('2026-09-15');
    expect(parseDate('091526', 'mdy-slash')).toBe('2026-09-15');
    // Unlike dmy/mdy, ISO 8601's own YYYYMMDD order has no 2-digit-year form to fall back to,
    // so 6 digits is ambiguous rather than a shorter version of the same pattern.
    expect(parseDate('202609', 'iso')).toBeNull();
  });
  it('rejects separator-free digits of any other length', () => {
    expect(parseDate('1234567', 'dmy-dot')).toBeNull();
    expect(parseDate('12345', 'dmy-dot')).toBeNull();
    expect(parseDate('1234567', 'iso')).toBeNull();
    expect(parseDate('12345', 'iso')).toBeNull();
  });
  it('rejects an impossible date typed without separators', () => {
    expect(parseDate('31022026', 'dmy-dot')).toBeNull();
  });
});

describe('resolveDateFormat', () => {
  it('keeps an explicit choice', () => {
    expect(resolveDateFormat('iso', 'de', ['de-DE'])).toBe('iso');
  });
  it('auto: German means dd.mm.yyyy, English follows the region', () => {
    expect(resolveDateFormat('auto', 'de', ['en-US'])).toBe('dmy-dot');
    expect(resolveDateFormat('auto', 'en', ['en-US'])).toBe('mdy-slash');
    expect(resolveDateFormat('auto', 'en', ['en'])).toBe('mdy-slash');
    expect(resolveDateFormat('auto', 'en', ['en-GB', 'en-US'])).toBe('dmy-slash');
    expect(resolveDateFormat('auto', 'en', ['de-DE', 'en-AU'])).toBe('dmy-slash');
  });
});

describe('datePlaceholder', () => {
  it('spells the pattern in the app language', () => {
    expect(datePlaceholder('dmy-dot', 'de')).toBe('TT.MM.JJJJ');
    expect(datePlaceholder('dmy-dot', 'en')).toBe('DD.MM.YYYY');
    expect(datePlaceholder('mdy-slash', 'en')).toBe('MM/DD/YYYY');
    expect(datePlaceholder('iso', 'de')).toBe('JJJJ-MM-TT');
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

describe('lastActivityLabel', () => {
  const today = '2026-09-14';
  it('says today, yesterday and days ago within a month', () => {
    expect(lastActivityLabel('2026-09-14', today, 'en')).toBe('today');
    expect(lastActivityLabel('2026-09-13', today, 'en')).toBe('yesterday');
    expect(lastActivityLabel('2026-09-11', today, 'en')).toBe('3 days ago');
    expect(lastActivityLabel('2026-08-15', today, 'en')).toBe('30 days ago');
  });
  it('gives month and year beyond a month', () => {
    expect(lastActivityLabel('2025-03-02', today, 'en')).toBe('Mar 2025');
    expect(lastActivityLabel('2025-03-02', today, 'de')).toMatch(/2025$/);
  });
  it('speaks the reader\'s language', () => {
    expect(lastActivityLabel('2026-09-11', today, 'de')).toMatch(/3/);
  });
  it('is empty without a date', () => {
    expect(lastActivityLabel(null, today, 'en')).toBe('');
  });
});
