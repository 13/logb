import { todayIso } from './format';
import { energyLabelKey } from './energy';
import type { Activity, ActivityInput, Category, FuelUnit, TitleSuggestion } from './types';

export function emptyActivity(): ActivityInput {
  return {
    date: todayIso(), category: 'maintenance', title: '', notes: '', counter_value: null, cost_cents: null, quantity_milli: null, tags: [],
    start_counter: null, from_place: null, to_place: null, duration_minutes: null, battery_used_pct: null,
  };
}

export function toActivityInput(a: Activity): ActivityInput {
  return {
    weight_grams: a.weight_grams ?? null, date: a.date, category: a.category, title: a.title, notes: a.notes, counter_value: a.counter_value, cost_cents: a.cost_cents, quantity_milli: a.quantity_milli, tags: [...a.tags],
    start_counter: a.start_counter, from_place: a.from_place, to_place: a.to_place, duration_minutes: a.duration_minutes, battery_used_pct: a.battery_used_pct,
  };
}

/** Returns the i18n key of the offending field, or null when valid. */
export function validateActivity(input: ActivityInput): string | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(input.date)) return 'activity.date';
  const isTrip = input.category === 'trip';
  // A trip defaults its own title to `cat.trip` when left blank (see ActivityForm), and a charge
  // (`fuel`) likewise defaults to "Charged"/"Geladen" or the petrol wording -- see
  // `activityTitle`. Every other category still needs one typed in.
  const titleOptional = isTrip || input.category === 'fuel' || input.category === 'weight';
  if (input.category === 'weight' && (!Number.isSafeInteger(input.weight_grams) || (input.weight_grams ?? 0) <= 0 || (input.weight_grams ?? 0) > 1_000_000_000)) return 'weight.invalid';
  if (!titleOptional && !input.title.trim()) return 'activity.title';
  if (input.cost_cents !== null && Number.isNaN(input.cost_cents)) return 'activity.cost';
  if (isTrip) {
    const start = input.start_counter;
    const end = input.counter_value;
    if (start === null || start === undefined || end === null) return 'trip.start';
    if (Number.isNaN(start) || Number.isNaN(end)) return 'trip.start';
    if (end < start) return 'trip.end';
    if (input.duration_minutes !== null && input.duration_minutes !== undefined && Number.isNaN(input.duration_minutes)) return 'trip.duration';
    const battery = input.battery_used_pct;
    if (battery !== null && battery !== undefined && (Number.isNaN(battery) || battery < 0 || battery > 100)) return 'trip.battery';
  } else if (input.counter_value !== null && Number.isNaN(input.counter_value)) {
    return 'activity.counter';
  }
  if (input.quantity_milli !== null && Number.isNaN(input.quantity_milli)) return 'activity.quantity';
  return null;
}

/** `[['2025', [...]], ['2024', [...]]]` — input is already newest-first from the API. */
export function groupByYear(list: Activity[]): Array<[string, Activity[]]> {
  const out: Array<[string, Activity[]]> = [];
  for (const a of list) {
    const year = a.date.slice(0, 4);
    const last = out[out.length - 1];
    if (last && last[0] === year) last[1].push(a);
    else out.push([year, [a]]);
  }
  return out;
}

export function exifDate(a: { taken_at: string | null }): string | null {
  return a.taken_at ? a.taken_at.slice(0, 10) : null;
}

/**
 * An activity's display title -- itself for anything but an untitled trip or charge, which fall
 * back to a word for what they are: `cat.trip` for a trip (the same word ActivityForm shows as
 * that field's own placeholder), or "Charged"/"Geladen" (`energy.charged-total`) vs "Fuel
 * logged"/"Getankt" (`insights.fuel-total`) for a charge, picked by `energyLabelKey` from the
 * object's fuel unit -- `fuelUnit` is optional exactly because not every caller (a global search
 * hit, a reminder's "link to activity" list) knows the owning object's, in which case this reads
 * as though it had none, the same generic wording `energyLabelKey(null)` already falls back to.
 *
 * `validateActivity` above is what makes an empty title mean "this is a trip or a charge"
 * everywhere: every other category is refused a blank one, so a blank `title` reaching any other
 * `category` here is not expected to happen -- it reads back as the empty string, same as it was
 * sent, rather than guessing at a fallback that was never asked for.
 *
 * Centralised so every place an activity's title is rendered agrees on the fallback, rather than
 * repeating the same three-way pick at each call site (a `LastDone` row, an `ActivityHit`, the
 * "link to activity" dropdown, the timeline itself, ...).
 */
export function activityTitle(title: string, category: Category | undefined, t: (key: string) => string, fuelUnit?: FuelUnit): string {
  if (title) return title;
  if (category === 'weight') return t('cat.weight');
  if (category === 'trip') return t('cat.trip');
  if (category === 'fuel') return t(energyLabelKey(fuelUnit ?? null) === 'energy.charged' ? 'energy.charged-total' : 'insights.fuel-total');
  return title;
}

/** Suggestions for the chosen category (all of them when none is chosen), one per title. An
 *  untitled trip has nothing worth repeating -- a "Repeat: " chip with nothing after the colon --
 *  so it is dropped here rather than only hidden by the template that renders these. */
export function suggestionsFor(all: TitleSuggestion[], category: Category | null): TitleSuggestion[] {
  const seen = new Set<string>();
  return all
    .filter((s) => s.title.trim() !== '')
    .filter((s) => category === null || s.category === category)
    .filter((s) => {
      if (seen.has(s.title)) return false;
      seen.add(s.title);
      return true;
    });
}
