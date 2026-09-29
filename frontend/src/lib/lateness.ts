import type { Reminder } from './types';

type Translate = (key: string, vars?: Record<string, string | number>) => string;

/** How late a due reminder is, when it is due by date: "due today", "1 day overdue", "12 days
 *  overdue". A counter-only reminder has no date to be late against, so it says nothing; its
 *  card already says it is due. Shared by the dashboard's reminder cards and the object page's
 *  summary. */
export function lateness(r: Pick<Reminder, 'days_until'>, t: Translate): string {
  if (r.days_until === null) return '';
  if (r.days_until === 0) return t('dash.due-today');
  if (r.days_until === -1) return t('dash.overdue-day');
  if (r.days_until < 0) return t('dash.overdue-days', { n: -r.days_until });
  return '';
}
