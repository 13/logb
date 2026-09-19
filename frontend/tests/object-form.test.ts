import { describe, it, expect } from 'vitest';
import { clearsPriceOn, emptyInput, toInput, validate } from '../src/lib/object-form';
import type { MemObject } from '../src/lib/types';

const obj = {
  id: 1, user_id: 1, name: 'Golf', type: 'car', counter_unit: 'km', fuel_unit: 'l', description: 'grey',
  purchase_date: '2020-03-01', purchase_price_cents: 1500000, archived_at: null,
  cover_attachment_id: null, cover_file_id: null, parent_id: null, created_at: '', updated_at: '', tags: ['Lease', 'Winter'],
  stats: { total_cost_cents: 0, activity_count: 0, current_counter: null, due_reminder_count: 0, last_reading_date: null },
  energy_price_milli: 30_000,
} as MemObject;

describe('object form', () => {
  it('maps an object to editable input', () => {
    expect(toInput(obj)).toEqual({
      weight_unit: 'kg',
      name: 'Golf', type: 'car', counter_unit: 'km', fuel_unit: 'l', description: 'grey',
      purchase_date: '2020-03-01', purchase_price_cents: 1500000, archived: false, parent_id: null, tags: ['Lease', 'Winter'],
      energy_price_milli: 30_000,
    });
    expect(toInput({ ...obj, archived_at: '2024-01-01T00:00:00Z' }).archived).toBe(true);
  });

  it('starts empty, defaulting to the catch-all type rather than the first in the list', () => {
    expect(emptyInput()).toEqual({
      weight_unit: 'kg',
      name: '', type: 'other', counter_unit: null, fuel_unit: null, description: '', purchase_date: null, purchase_price_cents: null,
      archived: false, parent_id: null, tags: [], energy_price_milli: null,
    });
  });

  it('requires a name', () => {
    expect(validate(emptyInput())).toBe('object.name');
    expect(validate({ ...emptyInput(), name: 'x' })).toBeNull();
    expect(validate({ ...emptyInput(), name: 'x', purchase_price_cents: NaN })).toBe('object.purchase-price');
  });

  it('rejects an unparseable energy price', () => {
    expect(validate({ ...emptyInput(), name: 'x', energy_price_milli: NaN })).toBe('object.energy-price-error');
    expect(validate({ ...emptyInput(), name: 'x', energy_price_milli: 30_000 })).toBeNull();
  });

  it('clears a kept price only when the fuel unit actually changes', () => {
    // Switching unit -- even between two non-null ones -- always clears: a price entered for
    // litres would otherwise silently misread as a per-kWh figure.
    expect(clearsPriceOn('l', 'kwh')).toBe(true);
    expect(clearsPriceOn('kwh', 'l')).toBe(true);
    expect(clearsPriceOn(null, 'l')).toBe(true);
    expect(clearsPriceOn('l', null)).toBe(true);
    // Re-selecting the same unit (or the field simply not having changed) keeps it.
    expect(clearsPriceOn('kwh', 'kwh')).toBe(false);
    expect(clearsPriceOn(null, null)).toBe(false);
  });
});
