import type { EveryUnit, Reminder, ReminderInput } from './types';

export function emptyReminder(): ReminderInput {
  return {
    title: '', notes: '', due_date: null, due_counter: null, repeat_months: null, repeat_counter: null,
    kind: 'service', every_n: null, every_unit: null, schedule: null,
  };
}

/** A reading reminder with the interval most people mean: once a month, starting from `start`. */
export function readingReminder(title: string, start: string | null = null): ReminderInput {
  return { ...emptyReminder(), title, kind: 'reading', every_n: 1, every_unit: 'month', due_date: start };
}

export function toReminderInput(r: Reminder): ReminderInput {
  return {
    title: r.title, notes: r.notes, due_date: r.due_date, due_counter: r.due_counter,
    repeat_months: r.repeat_months, repeat_counter: r.repeat_counter,
    kind: r.kind, every_n: r.every_n, every_unit: r.every_unit, schedule: r.schedule,
  };
}

/** Returns the i18n key of the offending field, or null when valid. */
export function validateReminder(input: ReminderInput): string | null {
  if (!input.title.trim()) return 'reminder.title';
  if (input.kind === 'reading') {
    if (input.every_n === null || Number.isNaN(input.every_n) || input.every_n < 1 || input.every_n > 60) return 'reminder.every';
    if (input.every_unit !== 'week' && input.every_unit !== 'month') return 'reminder.every';
    return null;
  }
  if (!input.schedule && !input.due_date && input.due_counter === null) return 'reminder.due-date';
  if (input.schedule && !/^(daily|weekly:[1-7]|monthly:(last|[1-9]|[12][0-9]|3[01])|yearly:(?:[1-9]|1[0-2]):(?:[1-9]|[12][0-9]|3[01]))$/.test(input.schedule)) return 'reminder.schedule';
  if (input.schedule && input.repeat_months !== null) return 'reminder.repeat-months';
  if (input.due_counter !== null && (Number.isNaN(input.due_counter) || input.due_counter < 0)) return 'reminder.due-counter';
  if (input.repeat_months !== null && (Number.isNaN(input.repeat_months) || input.repeat_months <= 0)) return 'reminder.repeat-months';
  if (input.repeat_counter !== null && (Number.isNaN(input.repeat_counter) || input.repeat_counter <= 0)) return 'reminder.repeat-counter';
  return null;
}

/** The body the server takes for this kind: a reading reminder sends no service fields and a
 *  service reminder no interval, since the server refuses either mix. */
export function reminderBody(input: ReminderInput): ReminderInput {
  if (input.kind === 'reading') {
    return { ...input, due_counter: null, repeat_months: null, repeat_counter: null, due_date: input.due_date || null, schedule: null };
  }
  return { ...input, every_n: null, every_unit: null };
}

export function splitReminders(list: Reminder[]): { due: Reminder[]; open: Reminder[]; done: Reminder[] } {
  return {
    due: list.filter((r) => !r.done_at && r.due),
    open: list.filter((r) => !r.done_at && !r.due),
    done: list.filter((r) => r.done_at !== null),
  };
}

/** Roughly how many days one interval is, for "skip this one" -- a snooze takes days. */
export function intervalDays(n: number | null, unit: EveryUnit | null): number {
  const days = (n ?? 1) * (unit === 'week' ? 7 : 30);
  return Math.min(Math.max(days, 1), 365);
}
