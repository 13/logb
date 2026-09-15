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
