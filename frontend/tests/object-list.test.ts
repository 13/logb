import { describe, expect, it } from 'vitest';
import { matchesQuery, parseSort, parseTab, sortObjects, visibleRows } from '../src/lib/object-list';
import type { MemObject, ObjectType } from '../src/lib/types';

let nextId = 1;
// `stats` is typed separately from the rest of `Partial<MemObject>`: intersecting two optional
// `stats` properties would collapse back to the full `ObjectStats` (TS keeps both sides'
// constraints), defeating the point of a partial override.
function obj(over: Omit<Partial<MemObject>, 'stats'> & { stats?: Partial<MemObject['stats']> } = {}): MemObject {
  const id = over.id ?? nextId++;
  return {
    id, user_id: 1, name: `Object ${id}`, type: 'other', counter_unit: null, fuel_unit: null, description: '',
    purchase_date: null, purchase_price_cents: null, archived_at: null, cover_attachment_id: null, cover_file_id: null,
    parent_id: null, created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    ...over,
    stats: {
      total_cost_cents: 0, activity_count: 0, current_counter: null, due_reminder_count: 0,
      last_reading_date: null, last_activity_date: null, counter_per_day_milli: null,
      ...over.stats,
    },
  } as MemObject;
}
const label = (t: ObjectType) => ({ e_bike: 'E-Bike', home: 'Zuhause' } as Record<string, string>)[t] ?? t;
const names = (list: { name: string }[]) => list.map((o) => o.name);

describe('parseSort / parseTab', () => {
  it('accepts only known values', () => {
    expect(parseSort('last-activity')).toBe('last-activity');
    expect(parseSort('nonsense')).toBeNull();
    expect(parseSort(null)).toBeNull();
    expect(parseTab('archived')).toBe('archived');
    expect(parseTab('whatever')).toBe('active');
  });
});

describe('matchesQuery', () => {
  const bike = obj({ name: 'Cube Kathmandu', type: 'e_bike', description: 'Pendeln zur Arbeit' });
  it('matches name, type label and description, ignoring case and accents', () => {
    expect(matchesQuery(bike, 'kathm', label)).toBe(true);
    expect(matchesQuery(bike, 'e-bike', label)).toBe(true);
    expect(matchesQuery(bike, 'ARBEIT', label)).toBe(true);
    expect(matchesQuery(obj({ name: 'Fahrräder' }), 'fahrrader', label)).toBe(true);
    expect(matchesQuery(bike, 'boiler', label)).toBe(false);
  });
  it('treats an empty or blank query as matching everything', () => {
    expect(matchesQuery(bike, '   ', label)).toBe(true);
  });
});

describe('sortObjects', () => {
  const a = obj({ name: 'alpha', updated_at: '2026-03-01T00:00:00Z', stats: { last_activity_date: '2026-02-01', total_cost_cents: 500, current_counter: 10 } });
  const b = obj({ name: 'Bravo', updated_at: '2026-05-01T00:00:00Z', stats: { last_activity_date: '2026-06-01', total_cost_cents: 0, current_counter: null } });
  const c = obj({ name: 'charlie', updated_at: '2026-04-01T00:00:00Z', stats: { last_activity_date: null, total_cost_cents: 900, current_counter: 20 } });
  const d = obj({ name: 'delta', updated_at: '2026-04-01T00:00:00Z', stats: { last_activity_date: null, total_cost_cents: 900, current_counter: 20 } });

  it('sorts names case-insensitively', () => {
    expect(names(sortObjects([c, b, a], 'name', 'en'))).toEqual(['alpha', 'Bravo', 'charlie']);
  });
  it('puts the newest activity first and objects without any last, by name', () => {
    expect(names(sortObjects([d, c, a, b], 'last-activity', 'en'))).toEqual(['Bravo', 'alpha', 'charlie', 'delta']);
  });
  it('puts the most recently changed first, ties by name', () => {
    expect(names(sortObjects([a, d, b, c], 'changed', 'en'))).toEqual(['Bravo', 'charlie', 'delta', 'alpha']);
  });
  it('puts the highest cost first and zero cost last', () => {
    expect(names(sortObjects([b, a, d, c], 'cost', 'en'))).toEqual(['charlie', 'delta', 'alpha', 'Bravo']);
  });
  it('puts the highest counter first and no counter last', () => {
    expect(names(sortObjects([b, a, d, c], 'counter', 'en'))).toEqual(['charlie', 'delta', 'alpha', 'Bravo']);
  });
  it('does not mutate its input', () => {
    const input = [c, b, a];
    sortObjects(input, 'name', 'en');
    expect(names(input)).toEqual(['charlie', 'Bravo', 'alpha']);
  });
});

describe('visibleRows', () => {
  const house = obj({ id: 100, name: 'House', type: 'home' });
  const boiler = obj({ id: 101, name: 'Boiler', parent_id: 100 });
  const shedArchived = obj({ id: 102, name: 'Shed', parent_id: 100, archived_at: '2026-01-01T00:00:00Z' });
  const mower = obj({ id: 103, name: 'Mower', parent_id: 102 }); // live child of an archived parent
  const car = obj({ id: 104, name: 'Car' });
  const active = [house, boiler, mower, car];
  const archived = [shedArchived];

  it('shows top-level objects on the active tab, counting a child of an archived parent as top-level', () => {
    const rows = visibleRows(active, archived, 'active', '', 'name', label, 'en');
    expect(rows.map((r) => [r.object.name, r.parentName])).toEqual([['Car', null], ['House', null], ['Mower', null]]);
  });
  it('searches every depth and names the parent of a nested match', () => {
    const rows = visibleRows(active, archived, 'active', 'boil', 'name', label, 'en');
    expect(rows.map((r) => [r.object.name, r.parentName])).toEqual([['Boiler', 'House']]);
  });
  it('names an archived parent too', () => {
    const rows = visibleRows(active, archived, 'active', 'mow', 'name', label, 'en');
    expect(rows.map((r) => r.parentName)).toEqual(['Shed']);
  });
  it('lists archived objects flat, with their parent', () => {
    const rows = visibleRows(active, archived, 'archived', '', 'name', label, 'en');
    expect(rows.map((r) => [r.object.name, r.parentName])).toEqual([['Shed', 'House']]);
  });
});
