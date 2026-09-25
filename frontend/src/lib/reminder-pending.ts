import { hashToNegativeId } from './activity-form';
import type { QueuedOp } from './outbox';
import type { Reminder, ReminderInput } from './types';

/**
 * Reminder creates still waiting in the outbox, shaped as reminders so the Reminders tab can
 * list them (with the pending chip) instead of the saved reminder vanishing until the next
 * successful connection. The id is derived from the op id, so it is negative -- never a real
 * reminder's -- and the same on every render.
 *
 * Only what the form sent is known; everything the server computes (due, days until, the
 * object's current counter) is left empty, which reads as "open".
 */
export function pendingReminders(ops: QueuedOp[], objectId: number): Reminder[] {
  return ops.filter((o) => o.kind === 'reminder.create').map((o) => {
    const b = o.body as unknown as ReminderInput;
    return {
      id: hashToNegativeId(o.id), object_id: objectId, title: b.title, notes: b.notes ?? '',
      due_date: b.due_date ?? null, due_counter: b.due_counter ?? null,
      repeat_months: b.repeat_months ?? null, repeat_counter: b.repeat_counter ?? null,
      done_at: null, done_activity_id: null, created_at: '', snoozed_until: null,
      object_name: '', counter_unit: null, current_counter: null, due: false,
      days_until: null, counter_until: null,
      kind: b.kind ?? 'service', every_n: b.every_n ?? null, every_unit: b.every_unit ?? null,
      schedule: b.schedule ?? null, last_reading_date: null, next_due_date: b.due_date ?? null,
      estimated_due_date: null, pending: true,
    };
  });
}
