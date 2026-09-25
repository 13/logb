import { describe, expect, it } from 'vitest';
import { pendingReminders } from '../src/lib/reminder-pending';
import { splitReminders } from '../src/lib/reminder-form';
import type { QueuedOp } from '../src/lib/outbox';

function op(id: string, kind: QueuedOp['kind'], body: Record<string, unknown>): QueuedOp {
  return { id, kind, path: '/objects/3/reminders', body, attempts: 0 };
}

describe('pendingReminders', () => {
  const body = { title: 'TÜV', notes: '', due_date: '2027-01-01', due_counter: null, repeat_months: 24, repeat_counter: null, kind: 'service', every_n: null, every_unit: null, schedule: null };

  it('shows a queued reminder create as an open, pending reminder of this object', () => {
    const [r] = pendingReminders([op('a', 'reminder.create', body)], 3);
    expect(r).toMatchObject({ object_id: 3, title: 'TÜV', due_date: '2027-01-01', repeat_months: 24, done_at: null, due: false, pending: true });
    expect(r.id).toBeLessThan(0);
    expect(splitReminders([r]).open).toEqual([r]);
  });

  it('gives each queued create its own stable id and ignores other kinds', () => {
    const ops = [op('a', 'reminder.create', body), op('b', 'reminder.create', body), op('c', 'activity.create', {})];
    const first = pendingReminders(ops, 3);
    expect(first).toHaveLength(2);
    expect(first[0].id).not.toBe(first[1].id);
    expect(pendingReminders(ops, 3).map((r) => r.id)).toEqual(first.map((r) => r.id));
  });
});
