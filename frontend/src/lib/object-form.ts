import type { MemObject, ObjectInput } from './types';

export function emptyInput(): ObjectInput {
  return { name: '', category: '', counter_unit: null, description: '', purchase_date: null, purchase_price_cents: null, archived: false };
}

export function toInput(o: MemObject): ObjectInput {
  return {
    name: o.name, category: o.category, counter_unit: o.counter_unit, description: o.description,
    purchase_date: o.purchase_date, purchase_price_cents: o.purchase_price_cents, archived: o.archived_at !== null,
  };
}

/** Returns the i18n key of the offending field, or null when valid. */
export function validate(input: ObjectInput): string | null {
  if (!input.name.trim()) return 'object.name';
  if (!input.category.trim()) return 'object.category';
  if (input.purchase_price_cents !== null && Number.isNaN(input.purchase_price_cents)) return 'object.purchase-price';
  return null;
}
