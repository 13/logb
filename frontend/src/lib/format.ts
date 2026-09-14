export function money(cents: number | null | undefined, currency: string, locale: string): string {
  if (cents === null || cents === undefined) return '';
  return new Intl.NumberFormat(locale, { style: 'currency', currency }).format(cents / 100);
}

/** Like `money`, but rounded to the nearest whole currency unit -- for an average (e.g. the
 *  per-year ownership figure), where cents claim a precision the figure does not have. */
export function moneyWhole(cents: number | null | undefined, currency: string, locale: string): string {
  if (cents === null || cents === undefined) return '';
  return new Intl.NumberFormat(locale, { style: 'currency', currency, minimumFractionDigits: 0, maximumFractionDigits: 0 }).format(cents / 100);
}

export function fmtDate(iso: string | null | undefined, locale: string): string {
  if (!iso) return '';
  const [y, m, d] = iso.slice(0, 10).split('-').map(Number);
  return new Intl.DateTimeFormat(locale, { year: 'numeric', month: locale === 'de' ? '2-digit' : 'short', day: locale === 'de' ? '2-digit' : 'numeric' })
    .format(new Date(Date.UTC(y, m - 1, d, 12)));
}

export function counter(value: number | null | undefined, unit: string | null, locale: string): string {
  if (value === null || value === undefined) return '';
  const n = new Intl.NumberFormat(locale).format(value);
  return unit ? `${n} ${unit}` : n;
}

export function todayIso(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** "12.50" / "12,50" / "12" → cents; empty → null; invalid → NaN */
export function parseMoney(s: string): number | null {
  const t = s.trim().replace(/\s/g, '').replace(',', '.');
  if (t === '') return null;
  const n = Number(t);
  return Number.isFinite(n) ? Math.round(n * 100) : NaN;
}

export function centsToInput(cents: number | null): string {
  return cents === null ? '' : (cents / 100).toFixed(2);
}

/** "41.3" / "41,3" / "41" → milli-units; empty → null; invalid → NaN */
export function parseQuantity(s: string): number | null {
  const t = s.trim().replace(/\s/g, '').replace(',', '.');
  if (t === '') return null;
  const n = Number(t);
  return Number.isFinite(n) ? Math.round(n * 1000) : NaN;
}

/**
 * Milli-cents per unit, rendered as money.
 *
 * The value is `cost_per_counter_milli` from the insights endpoint: cents-per-unit scaled
 * by 1000 (see `cost_per_counter_milli` in src/domain/insights.rs). Divide by 1000 to get
 * cents, then by 100 to get currency units -- 100_000 in total. This is a different scale
 * from `quantity`'s milli-units (divisor 1000): don't unify the two divisors.
 */
export function perCounter(milli: number | null | undefined, currency: string, locale: string): string {
  if (milli === null || milli === undefined) return '';
  return new Intl.NumberFormat(locale, { style: 'currency', currency }).format(milli / 100_000);
}

/** A milli-scaled amount with its unit: 41_300 -> "41.3 l". */
export function quantity(milli: number | null | undefined, unit: string, locale: string): string {
  if (milli === null || milli === undefined) return '';
  const n = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(milli / 1000);
  return `${n} ${unit}`;
}

/** When something last happened, as a person says it: "today", "yesterday", "3 days ago" within
 *  a month, then the month and year. Dates are whole days (`YYYY-MM-DD`), so the difference is
 *  counted in UTC days and no timezone can shift it by one. */
export function lastActivityLabel(date: string | null, today: string, locale: string): string {
  if (!date) return '';
  const day = (iso: string) => Date.UTC(Number(iso.slice(0, 4)), Number(iso.slice(5, 7)) - 1, Number(iso.slice(8, 10)));
  const days = Math.round((day(today) - day(date)) / 86_400_000);
  if (days >= 0 && days <= 30) return new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(-days, 'day');
  const [y, m] = date.split('-').map(Number);
  return new Intl.DateTimeFormat(locale, { month: 'short', year: 'numeric', timeZone: 'UTC' }).format(new Date(Date.UTC(y, m - 1, 15, 12)));
}
