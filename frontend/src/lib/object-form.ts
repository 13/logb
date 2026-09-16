import type { FuelUnit, MemObject, ObjectInput } from './types';

export function emptyInput(): ObjectInput {
  return {
    name: '', type: 'other', counter_unit: null, fuel_unit: null, description: '', purchase_date: null, purchase_price_cents: null,
    archived: false, parent_id: null, tags: [], energy_price_milli: null,
  };
}

export function toInput(o: MemObject): ObjectInput {
  return {
    name: o.name, type: o.type, counter_unit: o.counter_unit, fuel_unit: o.fuel_unit, description: o.description,
    purchase_date: o.purchase_date, purchase_price_cents: o.purchase_price_cents, archived: o.archived_at !== null,
    parent_id: o.parent_id, tags: [...o.tags], energy_price_milli: o.energy_price_milli,
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
  // A dedicated, placeholder-free key -- `fieldError` (../lib/form-error.ts) calls `t(key)` with
  // no `vars` to build "Check <field>: …", and `object.energy-price` (the field's own LABEL,
  // used with `{unit}` interpolated at the template) would leave the literal text "{unit}" in
  // that sentence with none supplied.
  if (input.energy_price_milli !== null && input.energy_price_milli !== undefined && Number.isNaN(input.energy_price_milli)) return 'object.energy-price-error';
  return null;
}

/**
 * Whether a price kept from the previous fuel unit should survive a change to `next` -- never: a
 * price is only meaningful against the unit it was entered for (a €/kWh figure would misread as
 * €/l after a switch to litres), so any actual change clears it. Pure so the object form's fuel
 * unit `onchange` can be exercised without a DOM (see ObjectForm.svelte's `setFuelUnit`).
 */
export function clearsPriceOn(prev: FuelUnit, next: FuelUnit): boolean {
  return prev !== next;
}
