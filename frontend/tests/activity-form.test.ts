import { describe, it, expect } from 'vitest';
import { emptyActivity, toActivityInput, validateActivity, groupByYear, exifDate } from '../src/lib/activity-form';
import type { Activity } from '../src/lib/types';

function a(id: number, date: string): Activity {
  return { id, object_id: 1, date, category: 'repair', title: `t${id}`, notes: '', counter_value: null, cost_cents: null, created_at: '', updated_at: '', attachments: [] };
}

describe('activity form', () => {
  it('defaults the date to today', () => {
    expect(emptyActivity().date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(emptyActivity().category).toBe('maintenance');
  });

  it('maps an activity to input', () => {
    const src = { ...a(1, '2024-01-01'), cost_cents: 500, counter_value: 12 };
    expect(toActivityInput(src)).toEqual({ date: '2024-01-01', category: 'repair', title: 't1', notes: '', counter_value: 12, cost_cents: 500 });
  });

  it('validates required fields', () => {
    const base = emptyActivity();
    expect(validateActivity({ ...base, title: '' })).toBe('activity.title');
    expect(validateActivity({ ...base, title: 'x', date: '' })).toBe('activity.date');
    expect(validateActivity({ ...base, title: 'x', cost_cents: NaN })).toBe('activity.cost');
    expect(validateActivity({ ...base, title: 'x' })).toBeNull();
  });

  it('groups by year, newest first', () => {
    const groups = groupByYear([a(1, '2025-06-01'), a(2, '2025-01-02'), a(3, '2024-11-01')]);
    expect(groups.map(([y, xs]) => [y, xs.length])).toEqual([['2025', 2], ['2024', 1]]);
  });

  it('reads a photo date', () => {
    expect(exifDate({ taken_at: '2024-05-01T12:00:00' })).toBe('2024-05-01');
    expect(exifDate({ taken_at: null })).toBeNull();
  });
});
