import { centsToInput } from './format';
import type { FuelUnit, MemObject, ObjectInput } from './types';

export function emptyInput(): ObjectInput {
  return {
    name: '', type: 'other', counter_unit: null, fuel_unit: null, description: '', purchase_date: null, purchase_price_cents: null,
    weight_unit: 'kg', archived: false, parent_id: null, tags: [], energy_price_milli: null, fuel_capacity_milli: null,
    resource_unit: null, resource_kind: null, measurement_mode: null, monthly_target_milli: null, low_level_pct: null, private: false,
  };
}

export function toInput(o: MemObject): ObjectInput {
  return {
    weight_unit: o.weight_unit ?? 'kg', name: o.name, type: o.type, counter_unit: o.counter_unit, fuel_unit: o.fuel_unit, description: o.description,
    purchase_date: o.purchase_date, purchase_price_cents: o.purchase_price_cents, archived: o.archived_at !== null,
    parent_id: o.parent_id, tags: [...o.tags], energy_price_milli: o.energy_price_milli, fuel_capacity_milli: o.fuel_capacity_milli,
    resource_unit: o.resource_unit ?? o.fuel_unit, resource_kind: o.resource_kind ?? null, measurement_mode: o.measurement_mode ?? null,
    monthly_target_milli: o.monthly_target_milli ?? null, low_level_pct: o.low_level_pct ?? null, private: o.private === 1,
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
  if (input.fuel_capacity_milli !== null && input.fuel_capacity_milli !== undefined && (!Number.isFinite(input.fuel_capacity_milli) || input.fuel_capacity_milli <= 0)) return 'object.fuel-capacity-error';
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

/** A x1000 amount (tank capacity, monthly target) as the text its field shows: `50000` → `"50"`,
 *  nothing → an empty field. */
export function milliToText(milli: number | null | undefined): string {
  return milli == null ? '' : String(milli / 1000);
}

export interface ObjectFormText { priceText: string; energyPriceText: string; capacityText: string; targetText: string }

/**
 * The object form's free-text fields, filled from an input (a loaded object, a kept draft, a
 * saved template). `energy_price_milli` is cents x1000 -- sub-cent precision the field can carry
 * -- but the form shows it as the same whole-cent money text every other amount uses
 * (`centsToInput`/`parseMoney`), so a fractional cent rounds to the nearest whole one here, just
 * as `purchase_price_cents` does.
 */
export function formText(input: ObjectInput): ObjectFormText {
  return {
    priceText: centsToInput(input.purchase_price_cents),
    energyPriceText: centsToInput(input.energy_price_milli == null ? null : Math.round(input.energy_price_milli / 1000)),
    capacityText: milliToText(input.fuel_capacity_milli),
    targetText: milliToText(input.monthly_target_milli),
  };
}

/**
 * What an object that only reached the offline outbox looks like until the server answers: the
 * create's body, under its negative temp id, flagged `pending`. One builder for everywhere a
 * queued create has to be shown (the form that queued it, the dashboard listing the queue), so
 * the two can never disagree about what such an object looks like.
 */
export function pendingObject(body: ObjectInput, opts: { tempId: number; userId?: number; now: string }): MemObject {
  const { tempId, userId, now } = opts;
  return {
    id: tempId, user_id: userId ?? 0, name: body.name, type: body.type,
    counter_unit: body.counter_unit, fuel_unit: body.fuel_unit, description: body.description,
    purchase_date: body.purchase_date, purchase_price_cents: body.purchase_price_cents,
    archived_at: body.archived ? now : null, cover_attachment_id: null, cover_file_id: null,
    parent_id: body.parent_id ?? null, created_at: now, updated_at: now, ancestors: [],
    tags: [...(body.tags ?? [])], energy_price_milli: body.energy_price_milli ?? null,
    fuel_capacity_milli: body.fuel_capacity_milli ?? null, weight_unit: body.weight_unit ?? 'kg',
    resource_unit: body.resource_unit ?? body.fuel_unit, resource_kind: body.resource_kind ?? null,
    measurement_mode: body.measurement_mode ?? null, monthly_target_milli: body.monthly_target_milli ?? null,
    low_level_pct: body.low_level_pct ?? null, private: body.private ? 1 : 0,
    stats: { total_cost_cents: 0, activity_count: 0, current_counter: null, latest_weight_grams: null, latest_weight_date: null,
      due_reminder_count: 0, last_reading_date: null, last_activity_date: null, counter_per_day_milli: null },
    pending: true,
  };
}

/** Whether an object uses anything the form keeps under "More details": the form opens that
 *  section for it, so nothing already filled in sits out of sight. */
export function objectHasDetails(input: ObjectInput): boolean {
  return !!input.resource_kind || input.parent_id != null || input.description.trim() !== ''
    || (input.tags ?? []).length > 0 || input.purchase_date != null || input.purchase_price_cents != null
    || !!input.archived || !!input.private;
}

/** The object form's field for each key `validate` returns, plus the type (checked by the form:
 *  a new object has no type until one is picked). */
export const OBJECT_FIELD_IDS: Readonly<Record<string, string>> = {
  'object.type': 'object-type',
  'weight.invalid': 'sw',
  'object.name': 'n',
  'object.purchase-price': 'pp',
  'object.energy-price-error': 'ep',
  'object.fuel-capacity-error': 'capacity',
};
