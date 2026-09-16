import { describe, it, expect } from 'vitest';
import { energyLabelKey, formatPerUnit, energyCost } from '../src/lib/energy';

describe('energyLabelKey', () => {
  it('picks the charged wording for kWh', () => {
    expect(energyLabelKey('kwh')).toBe('energy.charged');
  });
  it('picks the filled wording for litres and gallons', () => {
    expect(energyLabelKey('l')).toBe('energy.filled');
    expect(energyLabelKey('gal')).toBe('energy.filled');
  });
  it('falls back to the fill wording for a missing title/unit', () => {
    expect(energyLabelKey(null)).toBe('energy.filled');
  });
});

describe('formatPerUnit', () => {
  it('formats a distance-per-unit rate with one decimal', () => {
    expect(formatPerUnit(7300, 'kwh', 'km', 'en')).toBe('7.3 km/kwh');
    expect(formatPerUnit(7300, 'kwh', 'km', 'de')).toBe('7,3 km/kwh');
  });
  it('works the same for litres', () => {
    expect(formatPerUnit(16000, 'l', 'km', 'en')).toBe('16 km/l');
  });
});

describe('energyCost', () => {
  it('multiplies distance by the rate and renders it as money', () => {
    // 200 km at 30_000 (cents x1000 per km, i.e. 0.30 EUR/km): 200 * 30_000 = 6_000_000 (cents
    // x1000) -> perCounter divides by 100_000 -> 60.00 EUR.
    expect(energyCost(200, 30_000, 'EUR', 'en')).toBe('€60.00');
    // `de`'s currency format places a non-breaking space before the symbol -- normalised to a
    // plain space, the same way format.test.ts does for `money`.
    expect(energyCost(200, 30_000, 'EUR', 'de').replace(/ /g, ' ')).toBe('60,00 €');
  });
});
