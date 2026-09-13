/**
 * The quick reading form's checks. Pure, so they are tested without a component.
 *
 * Neither check blocks: an odometer really can be replaced (lower than the last reading) and a
 * road trip really can add a lot. Both ask the person to look again, because a missing or extra
 * digit is by far the likelier cause, and a wrong reading quietly skews every rate and estimate
 * that is built on it.
 */

/** How many times the recent daily rate a new reading may imply before it is questioned. */
export const IMPLAUSIBLE_FACTOR = 5;
/** Below this many units a jump is never questioned, whatever the rate says: 300 km in a day
 *  is an ordinary long drive even for a car that usually does 10. */
const ALWAYS_PLAUSIBLE = 300;

export type ReadingWarning = 'lower' | 'implausible' | null;

export interface ReadingContext {
  /** The object's highest reading so far. */
  lastCounter: number | null;
  /** The date of its newest reading, `YYYY-MM-DD`. */
  lastDate: string | null;
  /** Units per day ×1000 from the insights endpoint, when known. */
  ratePerDayMilli: number | null;
}

function daysBetween(from: string, to: string): number {
  const ms = Date.parse(`${to}T12:00:00Z`) - Date.parse(`${from}T12:00:00Z`);
  return Math.round(ms / 86_400_000);
}

export function readingWarning(value: number, date: string, ctx: ReadingContext): ReadingWarning {
  if (ctx.lastCounter === null) return null;
  if (value < ctx.lastCounter) return 'lower';
  if (ctx.ratePerDayMilli === null || ctx.ratePerDayMilli <= 0 || ctx.lastDate === null) return null;
  const delta = value - ctx.lastCounter;
  if (delta <= ALWAYS_PLAUSIBLE) return null;
  const days = Math.max(daysBetween(ctx.lastDate, date), 1);
  const expected = (ctx.ratePerDayMilli / 1000) * days;
  return delta > expected * IMPLAUSIBLE_FACTOR ? 'implausible' : null;
}

/** `iso` plus `months` calendar months, clamped to the last day of a shorter month -- the same
 *  rule the server uses (`Every::after` in src/domain/reminder.rs). */
export function addMonthsIso(iso: string, months: number): string {
  const [y, m, d] = iso.split('-').map(Number);
  const target = new Date(Date.UTC(y, m - 1 + months, 1));
  const lastDay = new Date(Date.UTC(target.getUTCFullYear(), target.getUTCMonth() + 1, 0)).getUTCDate();
  target.setUTCDate(Math.min(d, lastDay));
  return target.toISOString().slice(0, 10);
}

/** The body of the activity a reading is saved as. */
export function readingActivity(value: number, date: string, title: string) {
  return {
    date, category: 'reading' as const, title, notes: '',
    counter_value: value, cost_cents: null, quantity_milli: null,
  };
}
