import { todayIso } from './format';
import type { Activity, ActivityInput, Category, TitleSuggestion } from './types';

export function emptyActivity(): ActivityInput {
  return { date: todayIso(), category: 'maintenance', title: '', notes: '', counter_value: null, cost_cents: null, quantity_milli: null };
}

export function toActivityInput(a: Activity): ActivityInput {
  return { date: a.date, category: a.category, title: a.title, notes: a.notes, counter_value: a.counter_value, cost_cents: a.cost_cents, quantity_milli: a.quantity_milli };
}

/** Returns the i18n key of the offending field, or null when valid. */
export function validateActivity(input: ActivityInput): string | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(input.date)) return 'activity.date';
  if (!input.title.trim()) return 'activity.title';
  if (input.cost_cents !== null && Number.isNaN(input.cost_cents)) return 'activity.cost';
  if (input.counter_value !== null && Number.isNaN(input.counter_value)) return 'activity.counter';
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

/** Suggestions for the chosen category (all of them when none is chosen), one per title. */
export function suggestionsFor(all: TitleSuggestion[], category: Category | null): TitleSuggestion[] {
  const seen = new Set<string>();
  return all
    .filter((s) => category === null || s.category === category)
    .filter((s) => {
      if (seen.has(s.title)) return false;
      seen.add(s.title);
      return true;
    });
}
