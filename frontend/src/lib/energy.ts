/**
 * Pure helpers for charges and energy figures (see
 * docs/superpowers/specs/2026-09-16-charging-energy-design.md). No Svelte, no i18n: everything
 * here is unit-testable on its own, like ./trip.ts.
 */
import { perCounter } from './format';
import type { FuelUnit } from './types';

/**
 * The i18n key stem for wording that depends on the object's fuel unit: a kWh object "charges"
 * ("Log charge" / "Laden eintragen"), everything else -- litres, gallons, or no unit at all --
 * "fills" ("Log fill" / "Tanken eintragen"). `null` falls back to the fill wording too: an object
 * with no fuel unit yet still offers the generic phrasing rather than showing nothing.
 */
export function energyLabelKey(fuelUnit: FuelUnit): 'energy.charged' | 'energy.filled' {
  return fuelUnit === 'kwh' ? 'energy.charged' : 'energy.filled';
}

/**
 * The fuel unit the way a person reads it, rather than the lowercase code `fuel_unit` (and
 * `EnergyOut.unit`/`Insights.fuel.unit`, both copied straight from it) stores: "kwh" -> "kWh".
 * Litres and gallons are already fine as stored, so they pass through unchanged; `null` (no
 * fuel unit) becomes `''`, the same "nothing to show" convention `fmtDate`/`counter` use for a
 * missing value.
 */
export function fuelUnitLabel(u: string | null): string {
  if (u === 'kwh') return 'kWh';
  return u ?? '';
}

/**
 * `distance_per_unit_milli` (the counter distance per fuel unit, scaled by 1000, e.g. `7300` for
 * 7.3 km/kWh) as "7.3 km/kwh". One decimal place: the value is a mean rate over several windows,
 * and more digits would claim a precision it does not have (the same reasoning `formatSpeed` in
 * ./trip.ts uses).
 */
export function formatPerUnit(milli: number, unit: string, counterUnit: string, locale: string): string {
  const n = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(milli / 1000);
  return `${n} ${counterUnit}/${unit}`;
}

/**
 * A distance's energy cost at `costPerCounterMilli` (cents x1000 per counter unit, the same scale
 * `cost_per_counter_milli` and `perCounter` already use), rendered as money -- e.g. the trip
 * row's "≈ {money}" and the Trips table's "Energy cost" row. `distance x costPerCounterMilli`
 * keeps the same "cents x1000" scale as a single division away from money, so `perCounter` (which
 * already divides that scale down to currency units) reads it unchanged.
 */
export function energyCost(distance: number, costPerCounterMilli: number, currency: string, locale: string): string {
  return perCounter(distance * costPerCounterMilli, currency, locale);
}
