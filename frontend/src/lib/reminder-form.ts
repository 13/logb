import type { Reminder, ReminderInput } from './types';

export function emptyReminder(): ReminderInput {
  return { title: '', notes: '', due_date: null, due_counter: null, repeat_months: null, repeat_counter: null };
}

export function toReminderInput(r: Reminder): ReminderInput {
  return {
    title: r.title, notes: r.notes, due_date: r.due_date, due_counter: r.due_counter,
    repeat_months: r.repeat_months, repeat_counter: r.repeat_counter,
  };
}

/** Returns the i18n key of the offending field, or null when valid. */
export function validateReminder(input: ReminderInput): string | null {
  if (!input.title.trim()) return 'reminder.title';
  if (!input.due_date && input.due_counter === null) return 'reminder.due-date';
  if (input.due_counter !== null && (Number.isNaN(input.due_counter) || input.due_counter < 0)) return 'reminder.due-counter';
  if (input.repeat_months !== null && (Number.isNaN(input.repeat_months) || input.repeat_months <= 0)) return 'reminder.repeat-months';
  if (input.repeat_counter !== null && (Number.isNaN(input.repeat_counter) || input.repeat_counter <= 0)) return 'reminder.repeat-counter';
  return null;
}

export function splitReminders(list: Reminder[]): { due: Reminder[]; open: Reminder[]; done: Reminder[] } {
  return {
    due: list.filter((r) => !r.done_at && r.due),
    open: list.filter((r) => !r.done_at && !r.due),
    done: list.filter((r) => r.done_at !== null),
  };
}
