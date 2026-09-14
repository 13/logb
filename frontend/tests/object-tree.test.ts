import { describe, expect, it } from 'vitest';
import { excludingDescendants } from '../src/lib/object-tree';
import type { MemObject } from '../src/lib/types';

function obj(id: number, parent_id: number | null, archived_at: string | null = null): MemObject {
  return { id, user_id: 1, name: `#${id}`, type: 'other', counter_unit: null, fuel_unit: null,
    description: '', purchase_date: null, purchase_price_cents: null, archived_at,
    cover_attachment_id: null, cover_file_id: null, parent_id, created_at: '', updated_at: '',
    stats: { total_cost_cents: 0, activity_count: 0, current_counter: null, due_reminder_count: 0, last_reading_date: null,
      last_activity_date: null, counter_per_day_milli: null } };
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

  // House(1) holds Garage(2) holds Main light(3), and the garage is archived. Archiving a room
  // does not move the light out of the house, so the light is still the house's descendant and
  // still an illegal parent for it -- which is exactly what the server says when it walks the
  // real table. Nothing about being archived enters into it.
  it('excludes a descendant reachable only through an archived object', () => {
    const all = [obj(1, null), obj(2, 1, '2024-01-01T00:00:00Z'), obj(3, 2)];
    expect(excludingDescendants(all, 1).map((o) => o.id)).toEqual([]);
  });

  // The contract, stated as a test because breaking it is silent: the function reasons about
  // the list it is handed and nothing else. Hand it a list with the garage missing -- which is
  // precisely what a single `GET /objects?all=true` returns once the garage is archived -- and
  // the walk finds no children of the house, so the house's own grandchild comes back as a
  // legal parent. The caller merges both archived states so this case cannot arise; if this
  // test ever starts failing, someone has made the function guess at rows it was not given.
  it('can only see the links in the list it is given', () => {
    const withoutTheArchivedGarage = [obj(1, null), obj(3, 2)];
    expect(excludingDescendants(withoutTheArchivedGarage, 1).map((o) => o.id)).toEqual([3]);
  });
});
