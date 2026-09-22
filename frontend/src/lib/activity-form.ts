import { todayIso, centsToInput, parseMoney, parseQuantity } from './format';
import { energyLabelKey } from './energy';
import { parseWeight, weightInput } from './weight';
import { formatDuration, parseDuration, tripDistance } from './trip';
import { CATEGORIES } from './types';
import type { Activity, ActivityInput, Category, MemObject, ResourceUnit, TitleSuggestion, WeightUnit } from './types';

export function emptyActivity(): ActivityInput {
  return {
    date: todayIso(), category: 'maintenance', title: '', notes: '', counter_value: null, cost_cents: null, quantity_milli: null, tags: [],
    start_counter: null, from_place: null, to_place: null, duration_minutes: null, battery_used_pct: null, fuel_level_pct: null,
    meter_reading_milli: null, period_start: null, period_end: null, estimated: 0, meter_reset: 0,
  };
}

export function toActivityInput(a: Activity): ActivityInput {
  return {
    weight_grams: a.weight_grams ?? null, fuel_level_pct: a.fuel_level_pct ?? null, date: a.date, category: a.category, title: a.title, notes: a.notes, counter_value: a.counter_value, cost_cents: a.cost_cents, quantity_milli: a.quantity_milli, tags: [...a.tags],
    start_counter: a.start_counter, from_place: a.from_place, to_place: a.to_place, duration_minutes: a.duration_minutes, battery_used_pct: a.battery_used_pct,
    meter_reading_milli: a.meter_reading_milli ?? null, period_start: a.period_start ?? null, period_end: a.period_end ?? null,
    estimated: a.estimated ?? 0, meter_reset: a.meter_reset ?? 0,
  };
}

/** Returns the i18n key of the offending field, or null when valid. */
export function validateActivity(input: ActivityInput): string | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(input.date)) return 'activity.date';
  const isTrip = input.category === 'trip';
  // A trip defaults its own title to `cat.trip` when left blank (see ActivityForm), and a charge
  // (`fuel`) likewise defaults to "Charged"/"Geladen" or the petrol wording -- see
  // `activityTitle`. Every other category still needs one typed in.
  const titleOptional = isTrip || input.category === 'fuel' || input.category === 'usage' || input.category === 'weight';
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
  if (input.fuel_level_pct !== null && input.fuel_level_pct !== undefined && (!Number.isFinite(input.fuel_level_pct) || input.fuel_level_pct < 0 || input.fuel_level_pct > 100)) return 'activity.fuel-level-error';
  if (input.meter_reading_milli !== null && input.meter_reading_milli !== undefined && (!Number.isFinite(input.meter_reading_milli) || input.meter_reading_milli < 0)) return 'activity.meter-reading';
  if (input.period_start && input.period_end && input.period_start > input.period_end) return 'activity.period-start';
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
export function activityTitle(title: string, category: Category | undefined, t: (key: string) => string, fuelUnit?: ResourceUnit): string {
  if (title) return title;
  if (category === 'weight') return t('cat.weight');
  if (category === 'trip') return t('cat.trip');
  if (category === 'fuel') return t(energyLabelKey(fuelUnit ?? null) === 'energy.charged' ? 'energy.charged-total' : 'insights.fuel-total');
  if (category === 'usage') return t('cat.usage');
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

/** A stable negative id derived from a string (an outbox op id), so a queued row can sit in an
 *  `id`-keyed list beside real, always-positive ids. Shared by ActivityForm (minting a temp id)
 *  and ObjectDetail (rendering a queued op). */
export function hashToNegativeId(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
  return -(Math.abs(h) || 1);
}

/** Which category a new entry starts on: a `?category=` the object offers wins and counts as
 *  the user's pick; otherwise the current one if offered, else the first offered. */
export function resolveCategory(offered: Category[], wanted: Category | null, current: Category): { category: Category; touched: boolean } {
  if (wanted && offered.includes(wanted)) return { category: wanted, touched: true };
  if (!offered.includes(current)) return { category: offered[0], touched: false };
  return { category: current, touched: false };
}

/** The `?category=` of a `.../activities/new` link; unknown values are ignored. */
export function parseCategoryParam(search: string): Category | null {
  const c = new URLSearchParams(search).get('category');
  return c && (CATEGORIES as readonly string[]).includes(c) ? (c as Category) : null;
}

export function counterBelowLast(counterText: string, lastCounter: number | null): boolean {
  return counterText !== '' && lastCounter !== null && Number(counterText) < lastCounter;
}

export function weightDeviates(category: Category, weightText: string, unit: WeightUnit, latestGrams: number | null | undefined): boolean {
  if (category !== 'weight' || latestGrams == null) return false;
  const grams = parseWeight(weightText, unit);
  return Number.isFinite(grams) && Math.abs(grams - latestGrams) / latestGrams > 0.1;
}

export function firstExifDate(attachments: { taken_at: string | null }[]): string | null {
  return attachments.map(exifDate).find((d) => d !== null) ?? null;
}

export interface FormTexts {
  costText: string; counterText: string; quantityText: string; meterReadingText: string;
  fromText: string; toText: string; durationText: string; chargedFull: boolean;
}

/** The loaded row as the form's text fields show it. */
export function activityToFormText(a: Activity, weightUnit: WeightUnit): FormTexts & { weightText: string; distance: number | null } {
  return {
    weightText: weightInput(a.weight_grams, weightUnit),
    costText: centsToInput(a.cost_cents),
    counterText: a.counter_value === null ? '' : String(a.counter_value),
    quantityText: a.quantity_milli === null ? '' : String(a.quantity_milli / 1000),
    meterReadingText: a.meter_reading_milli == null ? '' : String(a.meter_reading_milli / 1000),
    chargedFull: a.charged_full === 1,
    fromText: a.from_place ?? '',
    toText: a.to_place ?? '',
    durationText: a.duration_minutes === null ? '' : formatDuration(a.duration_minutes),
    distance: tripDistance(a),
  };
}

export interface WeightState { text: string; unit: WeightUnit; originalText: string; originalUnit: WeightUnit; original: number | null }

/** Switching kg/lb converts the typed number without drifting: an untouched value goes back
 *  to its original grams rather than through two roundings. */
export function changeWeightUnitState(s: WeightState, next: WeightUnit): WeightState {
  const untouched = s.original !== null && s.text === s.originalText && s.unit === s.originalUnit;
  const grams = untouched ? s.original! : parseWeight(s.text, s.unit);
  if (!Number.isFinite(grams)) return { ...s, unit: next };
  const text = weightInput(grams, next);
  return { text, unit: next, originalText: text, originalUnit: next, original: grams };
}

function storedGrams(w: WeightState): number {
  const untouched = w.text === w.originalText && w.unit === w.originalUnit && w.original !== null;
  return untouched ? w.original! : parseWeight(w.text, w.unit);
}

/** The request body for the current form state: category-gated so a field typed under one
 *  category cannot ride along after switching to another. */
export function buildActivityInput(input: ActivityInput, texts: FormTexts, weight: WeightState, object: MemObject | null, t: (k: string) => string): ActivityInput {
  const cat = input.category;
  const isTrip = cat === 'trip';
  const isSession = cat === 'session';
  const isWeight = cat === 'weight';
  const isUsage = cat === 'usage';
  const resourceUnit = object?.resource_unit ?? object?.fuel_unit ?? null;
  const liquid = resourceUnit === 'l' || resourceUnit === 'gal';
  const water = object?.resource_kind === 'water';
  const meter = object?.measurement_mode === 'meter';
  const usageMode = object?.measurement_mode === 'usage';
  return {
    ...input,
    title: isWeight ? (input.title || t('cat.weight')) : input.title,
    weight_grams: isWeight ? storedGrams(weight) : null,
    // A plain copy: `input.tags` is a $state proxy, and IndexedDB cannot clone a proxy, so
    // queuing this body offline (the outbox) would fail with the spread's array left as it is.
    tags: [...(input.tags ?? [])],
    cost_cents: isWeight ? null : parseMoney(texts.costText),
    // A trip's end IS the counter (`input.counter_value` is bound straight to the End field
    // below, unlike the generic Counter field's own `counterText`); every other category keeps
    // reading the generic field, exactly as before.
    counter_value: isWeight ? null : isTrip ? input.counter_value : (String(texts.counterText).trim() === '' ? null : Number(texts.counterText)),
    // The quantity field only exists in the form for the fuel category (see the template
    // below) -- send it only then, so switching category away from fuel after typing an
    // amount can't leave a fuel quantity stuck on a repair/maintenance/... row.
    // Same comma/dot handling as parseMoney, so this field and cost agree on what's valid input.
    quantity_milli: cat === 'fuel' || (isUsage && !meter) ? parseQuantity(texts.quantityText) : null,
    // Same pattern as `quantity_milli` just above: 1 only while this IS a fuel entry and the
    // box is ticked, 0 otherwise -- so switching category away from fuel after ticking it
    // can't leave the flag stuck set on a repair/maintenance/... row (the backend rejects it
    // there outright: "only a charge can be marked full").
    charged_full: (cat === 'fuel' || (isUsage && !water)) && texts.chargedFull ? 1 : 0,
    fuel_level_pct: (cat === 'fuel' || isUsage) && liquid && !water ? (input.fuel_level_pct ?? null) : null,
    meter_reading_milli: isUsage && water && meter ? parseQuantity(texts.meterReadingText) : null,
    period_start: isUsage && usageMode ? (input.period_start || null) : null,
    period_end: isUsage && usageMode ? (input.period_end || null) : null,
    estimated: isUsage ? (input.estimated ?? 0) : 0,
    meter_reset: isUsage && meter ? (input.meter_reset ?? 0) : 0,
    // The five trip fields exist in the form only for the trip category (see the template
    // below) -- sent as null otherwise, mirroring `quantity_milli` above, so switching away
    // from trip after filling any of them in can't leave them stuck on a repair/maintenance/...
    // row (the backend rejects them there outright, and PATCH keeps whatever it last stored
    // when a field is merely absent from the body).
    start_counter: isTrip ? input.start_counter : null,
    from_place: isTrip || isSession ? (texts.fromText.trim() || null) : null,
    to_place: isTrip ? (texts.toText.trim() || null) : null,
    duration_minutes: isTrip || isSession ? parseDuration(texts.durationText) : null,
    battery_used_pct: isTrip ? input.battery_used_pct : null,
  };
}

/** The row shown while a create is still in the outbox. */
export function optimisticActivity(tempId: number, oid: number, body: ActivityInput): Activity {
  const now = new Date().toISOString();
  return {
    id: tempId, object_id: oid, ...body, tags: body.tags ?? [],
    start_counter: body.start_counter ?? null, from_place: body.from_place ?? null,
    to_place: body.to_place ?? null, duration_minutes: body.duration_minutes ?? null,
    battery_used_pct: body.battery_used_pct ?? null, charged_full: body.charged_full ?? 0, weight_grams: body.weight_grams ?? null,
    fuel_level_pct: body.fuel_level_pct ?? null,
    meter_reading_milli: body.meter_reading_milli ?? null, period_start: body.period_start ?? null, period_end: body.period_end ?? null,
    estimated: body.estimated ?? 0, meter_reset: body.meter_reset ?? 0,
    created_at: now, updated_at: now, attachments: [], pending: true,
  };
}
