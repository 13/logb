import { describe, it, expect } from 'vitest';
import { emptyInput, toInput, validate } from '../src/lib/object-form';
import type { MemObject } from '../src/lib/types';

const obj = {
  id: 1, user_id: 1, name: 'Golf', category: 'car', counter_unit: 'km', fuel_unit: 'l', description: 'grey',
  purchase_date: '2020-03-01', purchase_price_cents: 1500000, archived_at: null,
  cover_attachment_id: null, created_at: '', updated_at: '',
  stats: { total_cost_cents: 0, activity_count: 0, current_counter: null, due_reminder_count: 0 },
} as MemObject;

describe('object form', () => {
  it('maps an object to editable input', () => {
    expect(toInput(obj)).toEqual({
      name: 'Golf', category: 'car', counter_unit: 'km', fuel_unit: 'l', description: 'grey',
      purchase_date: '2020-03-01', purchase_price_cents: 1500000, archived: false,
    });
    expect(toInput({ ...obj, archived_at: '2024-01-01T00:00:00Z' }).archived).toBe(true);
  });

  it('starts empty', () => {
    expect(emptyInput()).toEqual({ name: '', category: '', counter_unit: null, fuel_unit: null, description: '', purchase_date: null, purchase_price_cents: null, archived: false });
  });

  it('requires name and category', () => {
    expect(validate(emptyInput())).toBe('object.name');
    expect(validate({ ...emptyInput(), name: 'x' })).toBe('object.category');
    expect(validate({ ...emptyInput(), name: 'x', category: 'car' })).toBeNull();
    expect(validate({ ...emptyInput(), name: 'x', category: 'car', purchase_price_cents: NaN })).toBe('object.purchase-price');
  });
});
