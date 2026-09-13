import type { MemObject, ObjectInput } from './types';

export function emptyInput(): ObjectInput {
  return { name: '', type: 'other', counter_unit: null, fuel_unit: null, description: '', purchase_date: null, purchase_price_cents: null, archived: false, parent_id: null };
}

export function toInput(o: MemObject): ObjectInput {
  return {
    name: o.name, type: o.type, counter_unit: o.counter_unit, fuel_unit: o.fuel_unit, description: o.description,
    purchase_date: o.purchase_date, purchase_price_cents: o.purchase_price_cents, archived: o.archived_at !== null,
    parent_id: o.parent_id,
  };
}

/** Returns the i18n key of the offending field, or null when valid.
 *
 * `type` needs no check here: it is a closed enum bound to a picker, never free text, so it
 * cannot be entered invalid the way a free-text category could.
 */
export function validate(input: ObjectInput): string | null {
  if (!input.name.trim()) return 'object.name';
  if (input.purchase_price_cents !== null && Number.isNaN(input.purchase_price_cents)) return 'object.purchase-price';
  return null;
}
