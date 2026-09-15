/**
 * Pure helpers for the trip log (see docs/superpowers/specs/2026-09-15-trip-log-design.md).
 * No Svelte, no i18n, no API: everything here is unit-testable on its own.
 */

/**
 * "h:mm" or plain minutes -> minutes. Empty input means "not entered" (`null`); anything that
 * does not parse, or parses to a duration outside the backend's 1-10080 minute range, is `NaN`
 * -- distinct from `null` so a form can tell "nothing typed" from "typed something wrong".
 */
export function parseDuration(text: string): number | null {
  const trimmed = text.trim();
  if (trimmed === '') return null;
  let minutes: number;
  const hm = trimmed.match(/^(\d+):(\d{1,2})$/);
  if (hm) {
    const mm = Number(hm[2]);
    if (mm > 59) return NaN; // "1:75" -- no such minute in an hour
    minutes = Number(hm[1]) * 60 + mm;
  } else if (/^\d+$/.test(trimmed)) {
    minutes = Number(trimmed);
  } else {
    return NaN;
  }
  return minutes >= 1 && minutes <= 10_080 ? minutes : NaN;
}

/** Minutes -> "h:mm", the same shape `parseDuration` reads back (75 -> "1:15"). */
export function formatDuration(minutes: number): string {
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return `${h}:${String(m).padStart(2, '0')}`;
}

/** A trip's distance, the same `counter_value - start_counter` the server computes and never
 *  stores; `null` when either side is not known yet (a form still being filled in, or an
 *  activity that -- being no trip at all -- carries no `start_counter`). */
export function tripDistance(a: { start_counter?: number | null; counter_value: number | null }): number | null {
  if (a.start_counter === null || a.start_counter === undefined || a.counter_value === null) return null;
  return a.counter_value - a.start_counter;
}

/** "Home → Office", "Home →", "→ Office", or "" when neither place is set. */
export function placesLabel(from: string | null | undefined, to: string | null | undefined): string {
  const f = from?.trim() ?? '';
  const t = to?.trim() ?? '';
  if (!f && !t) return '';
  if (f && t) return `${f} → ${t}`;
  return f ? `${f} →` : `→ ${t}`;
}

/**
 * "400 km → 600 km" -- Timeline's trip row joins its already-formatted start and end readings
 * with this same arrow. Kept here, not as inline template text in `Timeline.svelte`, so the
 * literal `→` lives in a `.ts` file: `tests/icons.test.ts` scans every `.svelte` file's own
 * source for a stray glyph standing in for an icon, and a bare arrow typed straight into a
 * template would trip that scan even though it is ordinary punctuation here, not an icon.
 */
export function spanLabel(from: string, to: string): string {
  return `${from} → ${to}`;
}

/** An average speed (`TripSummary`'s `speed_x10`, tenths of the object's own distance unit per
 *  hour) as "16.0 km/h" / "16,0 km/h" or "9.5 mph". Always one decimal: the value is already a
 *  rounded average (see `totals` in src/domain/trips.rs), so a bare `Intl` default -- which would
 *  drop a trailing ".0" -- would make a whole-number average look like an integer reading rather
 *  than the average it is. */
export function formatSpeed(speedX10: number, unit: 'km' | 'mi', locale: string): string {
  const n = new Intl.NumberFormat(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(speedX10 / 10);
  return `${n} ${unit === 'mi' ? 'mph' : 'km/h'}`;
}

/** `TripSummary`'s `distance_per_10pct` -- distance covered per 10 percentage points of battery
 *  used -- as "59 km / 10 %" / "59 km / 10 %". `unit` is the object's own distance unit
 *  (`counter_unit`, "km" or "mi"), the same as everywhere else a distance is shown. */
export function formatPer10Pct(value: number, unit: string, locale: string): string {
  return `${new Intl.NumberFormat(locale).format(value)} ${unit} / 10 %`;
}

/** The trip form's three linked counter fields, as ActivityForm holds them: `start`/`end` travel
 *  to the server (`start_counter`/`counter_value`); `distance` is local-only. */
export interface TripLink { start: number | null; end: number | null; distance: number | null }

/**
 * End typed directly: distance follows it (`end - start`). Clearing End -- typing it down to
 * nothing -- clears distance too, rather than leaving it showing a span that no longer has an
 * end: there is nothing left to describe a distance _of_.
 */
export function linkTripEnd(f: TripLink): TripLink {
  if (f.end === null) return { ...f, distance: null };
  return f.start === null ? f : { ...f, distance: f.end - f.start };
}

/** Distance typed directly: end follows it (`start + distance`), unless start is not known yet
 *  (nothing to add the distance onto). */
export function linkTripDistance(f: TripLink): TripLink {
  return f.start === null || f.distance === null ? f : { ...f, end: f.start + f.distance };
}

/** Start changed: an already-known distance is kept and end moves with it (`start + distance`);
 *  with no distance yet -- a start typed or prefilled before any end -- there is nothing to
 *  move, so this instead derives distance from whatever end is already there, if any. */
export function linkTripStart(f: TripLink): TripLink {
  if (f.start === null) return f;
  if (f.distance !== null) return { ...f, end: f.start + f.distance };
  return f.end === null ? f : { ...f, distance: f.end - f.start };
}
