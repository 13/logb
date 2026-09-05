import { describe, it, expect } from 'vitest';
import { emptyReminder, toReminderInput, validateReminder, splitReminders } from '../src/lib/reminder-form';
import type { Reminder } from '../src/lib/types';

function r(id: number, over: Partial<Reminder> = {}): Reminder {
  return {
    id, object_id: 1, title: `r${id}`, notes: '', due_date: '2030-01-01', due_counter: null,
    repeat_months: null, repeat_counter: null, done_at: null, done_activity_id: null, created_at: '',
    object_name: 'Golf', counter_unit: 'km', current_counter: null, due: false, ...over,
  };
}

describe('reminder form', () => {
  it('starts empty', () => {
    expect(emptyReminder()).toEqual({ title: '', notes: '', due_date: null, due_counter: null, repeat_months: null, repeat_counter: null });
  });

  it('maps a reminder to input', () => {
    expect(toReminderInput(r(1, { due_counter: 5000, repeat_months: 12 }))).toEqual({
      title: 'r1', notes: '', due_date: '2030-01-01', due_counter: 5000, repeat_months: 12, repeat_counter: null,
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

  it('splits due, open and done', () => {
    const list = [r(1, { due: true }), r(2), r(3, { done_at: '2026-01-01T00:00:00Z' })];
    const s = splitReminders(list);
    expect(s.due.map((x) => x.id)).toEqual([1]);
    expect(s.open.map((x) => x.id)).toEqual([2]);
    expect(s.done.map((x) => x.id)).toEqual([3]);
  });
});
