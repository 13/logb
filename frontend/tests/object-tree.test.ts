import { describe, expect, it } from 'vitest';
import { excludingDescendants } from '../src/lib/object-tree';
import type { MemObject } from '../src/lib/types';

function obj(id: number, parent_id: number | null): MemObject {
  return { id, user_id: 1, name: `#${id}`, type: 'other', counter_unit: null, fuel_unit: null,
    description: '', purchase_date: null, purchase_price_cents: null, archived_at: null,
    cover_attachment_id: null, cover_file_id: null, parent_id, created_at: '', updated_at: '',
    stats: { total_cost_cents: 0, activity_count: 0, current_counter: null, due_reminder_count: 0 } };
}

describe('excludingDescendants', () => {
  it('excludes nothing when creating a brand-new object', () => {
    const all = [obj(1, null), obj(2, 1)];
    expect(excludingDescendants(all, null).map((o) => o.id)).toEqual([1, 2]);
  });

  it('excludes the object itself', () => {
    const all = [obj(1, null), obj(2, null)];
    expect(excludingDescendants(all, 1).map((o) => o.id)).toEqual([2]);
  });

  it('excludes a direct child, so a room cannot be filed inside its own fixture', () => {
    const all = [obj(1, null), obj(2, 1)];
    expect(excludingDescendants(all, 1).some((o) => o.id === 2)).toBe(false);
  });

  it('excludes a grandchild, three levels deep', () => {
    const all = [obj(1, null), obj(2, 1), obj(3, 2)];
    expect(excludingDescendants(all, 1).map((o) => o.id)).toEqual([]);
  });

  it('does not exclude an unrelated object, even one that is also a leaf', () => {
    const all = [obj(1, null), obj(2, 1), obj(3, null)];
    expect(excludingDescendants(all, 1).map((o) => o.id)).toEqual([3]);
  });
});
