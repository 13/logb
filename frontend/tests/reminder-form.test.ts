import { describe, it, expect } from 'vitest';
import {
  emptyReminder, intervalDays, readingReminder, reminderBody, splitReminders, toReminderInput, validateReminder,
} from '../src/lib/reminder-form';
import type { Reminder } from '../src/lib/types';

function r(id: number, over: Partial<Reminder> = {}): Reminder {
  return {
    id, object_id: 1, title: `r${id}`, notes: '', due_date: '2030-01-01', due_counter: null,
    repeat_months: null, repeat_counter: null, done_at: null, done_activity_id: null, created_at: '',
    snoozed_until: null, object_name: 'Golf', counter_unit: 'km', current_counter: null, due: false,
    days_until: null, counter_until: null, kind: 'service', every_n: null, every_unit: null,
    last_reading_date: null, next_due_date: '2030-01-01', estimated_due_date: null, ...over,
  };
}

describe('reminder form', () => {
  it('starts empty, as a service reminder', () => {
    expect(emptyReminder()).toEqual({
      title: '', notes: '', due_date: null, due_counter: null, repeat_months: null, repeat_counter: null,
      kind: 'service', every_n: null, every_unit: null,
    });
  });

  it('maps a reminder to input', () => {
    expect(toReminderInput(r(1, { due_counter: 5000, repeat_months: 12 }))).toEqual({
      title: 'r1', notes: '', due_date: '2030-01-01', due_counter: 5000, repeat_months: 12, repeat_counter: null,
      kind: 'service', every_n: null, every_unit: null,
    });
    expect(toReminderInput(r(2, { kind: 'reading', every_n: 2, every_unit: 'week' }))).toMatchObject({
      kind: 'reading', every_n: 2, every_unit: 'week',
    });
  });

  it('requires a title and one due condition', () => {
    expect(validateReminder(emptyReminder())).toBe('reminder.title');
    expect(validateReminder({ ...emptyReminder(), title: 'x' })).toBe('reminder.due-date');
    expect(validateReminder({ ...emptyReminder(), title: 'x', due_date: '2030-01-01' })).toBeNull();
    expect(validateReminder({ ...emptyReminder(), title: 'x', due_counter: 100 })).toBeNull();
    expect(validateReminder({ ...emptyReminder(), title: 'x', due_counter: 100, repeat_months: 0 })).toBe('reminder.repeat-months');
    expect(validateReminder({ ...emptyReminder(), title: 'x', due_counter: 100, repeat_counter: -5 })).toBe('reminder.repeat-counter');
  });

  it('asks a reading reminder for an interval instead of a due condition', () => {
    const reading = readingReminder('Log km');
    expect(validateReminder(reading)).toBeNull();
    expect(validateReminder({ ...reading, every_n: 0 })).toBe('reminder.every');
    expect(validateReminder({ ...reading, every_n: 61 })).toBe('reminder.every');
    expect(validateReminder({ ...reading, every_n: NaN })).toBe('reminder.every');
    expect(validateReminder({ ...reading, every_unit: null })).toBe('reminder.every');
  });

  it('sends only the fields the kind takes', () => {
    // Switching kind in the form leaves the other kind's fields typed in; the server refuses a mix.
    const mixed = { ...readingReminder('Log km'), due_counter: 5000, repeat_months: 3 };
    expect(reminderBody(mixed)).toMatchObject({ kind: 'reading', due_counter: null, repeat_months: null, every_n: 1 });
    const service = { ...emptyReminder(), title: 'Oil', due_date: '2030-01-01', every_n: 1, every_unit: 'month' as const };
    expect(reminderBody(service)).toMatchObject({ kind: 'service', every_n: null, every_unit: null });
  });

  it('skips roughly one interval', () => {
    expect(intervalDays(1, 'month')).toBe(30);
    expect(intervalDays(2, 'week')).toBe(14);
    expect(intervalDays(60, 'month')).toBe(365);
  });

  it('splits due, open and done', () => {
    const list = [r(1, { due: true }), r(2), r(3, { done_at: '2026-01-01T00:00:00Z' })];
    const s = splitReminders(list);
    expect(s.due.map((x) => x.id)).toEqual([1]);
    expect(s.open.map((x) => x.id)).toEqual([2]);
    expect(s.done.map((x) => x.id)).toEqual([3]);
  });
});
