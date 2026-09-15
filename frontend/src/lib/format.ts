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

export type DateFormat = 'dmy-dot' | 'dmy-slash' | 'mdy-slash' | 'iso';
export type DateFormatPref = 'auto' | DateFormat;
export const DATE_FORMATS: DateFormatPref[] = ['auto', 'dmy-dot', 'dmy-slash', 'mdy-slash', 'iso'];

const pad = (n: number) => String(n).padStart(2, '0');

/** Pure string work on the calendar date: no Intl and no timezone, so a stored day can never
 *  shift by one and every device shows the same digits for the same choice. */
export function fmtDate(iso: string | null | undefined, format: DateFormat): string {
  if (!iso) return '';
  const [y, m, d] = iso.slice(0, 10).split('-');
  switch (format) {
    case 'dmy-dot': return `${d}.${m}.${y}`;
    case 'dmy-slash': return `${d}/${m}/${y}`;
    case 'mdy-slash': return `${m}/${d}/${y}`;
    case 'iso': return `${y}-${m}-${d}`;
  }
}

/**
 * Reads the chosen pattern back, leniently: `.`/`/`/`-`/`,`/single-space separators, 1-digit
 * day/month, and a 2-digit year as 20YY. Also reads separator-free digits in the format's own
 * field order (8 digits with a 4-digit year, or 6 with a 2-digit one) -- the keypad an iOS text
 * field offers for `inputmode="numeric"` has no `.`/`-`/`/` key at all, so typing a date there
 * has to work without one. `iso`'s 6-digit form is rejected rather than guessed at: unlike
 * `dmy`/`mdy`, a 2-digit year does not belong anywhere in ISO 8601's own YYYYMMDD order, and
 * `202609` (6 digits) could otherwise be misread as either "2026, day 09 of an implied month" or
 * a 2-digit-year form nothing else in the format supports. Rejects malformed and
 * calendar-impossible dates (31.02.) rather than silently clamping them.
 */
export function parseDate(text: string, format: DateFormat): string | null {
  const trimmed = text.trim();
  let a: number, b: number, c: number, yearDigits: number;

  if (/^\d+$/.test(trimmed)) {
    if (format === 'iso') {
      if (trimmed.length !== 8) return null;
      a = Number(trimmed.slice(0, 4)); b = Number(trimmed.slice(4, 6)); c = Number(trimmed.slice(6, 8));
      yearDigits = 4;
    } else if (trimmed.length === 8) {
      a = Number(trimmed.slice(0, 2)); b = Number(trimmed.slice(2, 4)); c = Number(trimmed.slice(4, 8));
      yearDigits = 4;
    } else if (trimmed.length === 6) {
      a = Number(trimmed.slice(0, 2)); b = Number(trimmed.slice(2, 4)); c = Number(trimmed.slice(4, 6));
      yearDigits = 2;
    } else {
      return null; // notably: 5 and 7 digits, both ambiguous, are neither of the two widths above
    }
  } else {
    const parts = trimmed.split(/[./,\-\s]/);
    if (parts.length !== 3 || parts.some((p) => !/^\d+$/.test(p))) return null;
    [a, b, c] = parts.map(Number);
    yearDigits = (format === 'iso' ? parts[0] : parts[2]).length;
    if (yearDigits !== 2 && yearDigits !== 4) return null;
  }

  let y: number, m: number, d: number;
  if (format === 'iso') [y, m, d] = [a, b, c];
  else if (format === 'mdy-slash') [m, d, y] = [a, b, c];
  else [d, m, y] = [a, b, c];
  if (yearDigits === 2) y += 2000;
  const date = new Date(Date.UTC(y, m - 1, d));
  if (date.getUTCFullYear() !== y || date.getUTCMonth() !== m - 1 || date.getUTCDate() !== d) return null;
  return `${y}-${pad(m)}-${pad(d)}`;
}

/**
 * `auto` resolves without Intl: German always means `dmy-dot`; English follows the browser's
 * region -- `en-US` (or no `en-*` tag at all) means `mdy-slash`, any other `en-XX` region means
 * `dmy-slash`. An explicit (non-`auto`) preference is returned unchanged.
 */
export function resolveDateFormat(pref: DateFormatPref, locale: 'en' | 'de', languages: readonly string[]): DateFormat {
  if (pref !== 'auto') return pref;
  if (locale === 'de') return 'dmy-dot';
  const tag = languages.map((l) => l.split('-')).find(([lang, region]) => lang.toLowerCase() === 'en' && region);
  return !tag || tag[1].toUpperCase() === 'US' ? 'mdy-slash' : 'dmy-slash';
}

/** The pattern spelled out in the app's own language, for a field's placeholder and its error
 *  hint -- "DD.MM.YYYY", "TT.MM.JJJJ", and so on. */
export function datePlaceholder(format: DateFormat, locale: 'en' | 'de'): string {
  const [D, M, Y] = locale === 'de' ? ['TT', 'MM', 'JJJJ'] : ['DD', 'MM', 'YYYY'];
  switch (format) {
    case 'dmy-dot': return `${D}.${M}.${Y}`;
    case 'dmy-slash': return `${D}/${M}/${Y}`;
    case 'mdy-slash': return `${M}/${D}/${Y}`;
    case 'iso': return `${Y}-${M}-${D}`;
  }
}

export function counter(value: number | null | undefined, unit: string | null, locale: string): string {
  if (value === null || value === undefined) return '';
  const n = new Intl.NumberFormat(locale).format(value);
  return unit ? `${n} ${unit}` : n;
}

/** Counter distance since the last time; null when either side is unknown or the counter went
 *  backwards (a replaced odometer, a typo) -- a negative "since" would only mislead. */
export function sinceCounter(current: number | null | undefined, last: number | null): number | null {
  if (current === null || current === undefined || last === null) return null;
  const d = current - last;
  return d >= 0 ? d : null;
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
