/** The request for an object's insights. `contents` is left out when off, so the common case is the plain path. */
export function insightsPath(objectId: number, contents: boolean): string {
  return contents ? `/objects/${objectId}/insights?contents=true` : `/objects/${objectId}/insights`;
}

/** Noon UTC on the day, so no timezone moves the date across midnight. */
function utc(y: number, m: number, d: number): Date {
  return new Date(Date.UTC(y, m - 1, d, 12));
}

/** `2024-05-01` as "May 2024". */
export function sinceLabel(date: string, locale: string): string {
  const [y, m] = date.split('-').map(Number);
  return new Intl.DateTimeFormat(locale, { month: 'short', year: 'numeric', timeZone: 'UTC' }).format(utc(y, m, 1));
}

/** `2026-09` as "Sep 26". The year stays, unlike on the Statistics screen: the last twelve months
 *  cross a year, and two bars both labelled "Sep" would be ambiguous. */
export function monthLabel(month: string, locale: string): string {
  const [y, m] = month.split('-').map(Number);
  return new Intl.DateTimeFormat(locale, { month: 'short', year: '2-digit', timeZone: 'UTC' }).format(utc(y, m, 15));
}

/** `2026-01-10` as "Jan 10". */
export function fillLabel(date: string, locale: string): string {
  const [y, m, d] = date.split('-').map(Number);
  return new Intl.DateTimeFormat(locale, { day: 'numeric', month: 'short', timeZone: 'UTC' }).format(utc(y, m, d));
}
