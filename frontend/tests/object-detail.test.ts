import { describe, it, expect } from 'vitest';
import { tagParam, offersTrip, offersEnergy, resourceCategory, foldTitle, pendingToActivity, filterPendingOps, nextUrl } from '../src/lib/object-detail';
import type { QueuedOp } from '../src/lib/outbox';

function op(id: string, body: Record<string, unknown>): QueuedOp {
  return { id, kind: 'activity.create', path: '/objects/1/activities', body, attempts: 0 };
}

describe('tagParam', () => {
  it('trims and treats blank as absent', () => {
    expect(tagParam('?tag=%20oil%20')).toBe('oil');
    expect(tagParam('?tag=')).toBeNull();
    expect(tagParam('')).toBeNull();
  });
});

describe('what an object offers', () => {
  it('trips need a distance unit', () => {
    expect(offersTrip({ counter_unit: 'km' } as never)).toBe(true);
    expect(offersTrip({ counter_unit: 'h' } as never)).toBe(false);
    expect(offersTrip(null)).toBe(false);
  });
  it('energy needs a fuel or resource unit', () => {
    expect(offersEnergy({ fuel_unit: 'kWh', resource_unit: undefined } as never)).toBe(true);
    expect(offersEnergy({ fuel_unit: null, resource_unit: 'l' } as never)).toBe(true);
    expect(offersEnergy({ fuel_unit: null } as never)).toBe(false);
  });
  it('a resource logs usage, everything else fuel', () => {
    expect(resourceCategory({ resource_kind: 'water' } as never)).toBe('usage');
    expect(resourceCategory({} as never)).toBe('fuel');
  });
});

describe('pendingToActivity', () => {
  it('maps a full body and marks the row pending with a negative id', () => {
    const a = pendingToActivity(op('x', { date: '2026-01-01', category: 'trip', title: 't', notes: 'n', counter_value: 600, start_counter: 400, from_place: 'A', to_place: 'B', duration_minutes: 30, battery_used_pct: 5, charged_full: 1, fuel_level_pct: 50, meter_reading_milli: 10, period_start: '2026-01-01', period_end: '2026-01-31', estimated: 1, meter_reset: 1, tags: ['a'], cost_cents: 5, quantity_milli: 7, weight_grams: 8 }), 9);
    expect(a.id).toBeLessThan(0);
    expect(a).toMatchObject({ object_id: 9, pending: true, category: 'trip', title: 't', notes: 'n', counter_value: 600, start_counter: 400, from_place: 'A', to_place: 'B', duration_minutes: 30, battery_used_pct: 5, charged_full: 1, fuel_level_pct: 50, meter_reading_milli: 10, period_start: '2026-01-01', period_end: '2026-01-31', estimated: 1, meter_reset: 1, tags: ['a'], cost_cents: 5, quantity_milli: 7, weight_grams: 8, attachments: [] });
  });
  it('defaults every wrongly typed field', () => {
    const a = pendingToActivity(op('y', { date: 5, category: undefined, title: 1, tags: 'nope', charged_full: 'x' }), 1);
    expect(a).toMatchObject({ category: 'other', title: '', notes: '', counter_value: null, charged_full: 0, tags: [], estimated: 0, meter_reset: 0 });
    expect(a.date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});

describe('filterPendingOps', () => {
  const ops = [op('1', { category: 'repair', title: ' Oil ', tags: ['Fahrräder'] }), op('2', { category: 'fuel', title: 'x', tags: [] })];
  it('matches category, folded tag and folded title', () => {
    expect(filterPendingOps(ops, { category: 'repair', tagFilter: null, titleFilter: null }).map((o) => o.id)).toEqual(['1']);
    expect(filterPendingOps(ops, { category: null, tagFilter: 'fahrrader', titleFilter: null }).map((o) => o.id)).toEqual(['1']);
    expect(filterPendingOps(ops, { category: null, tagFilter: null, titleFilter: 'oil' }).map((o) => o.id)).toEqual(['1']);
    expect(filterPendingOps(ops, { category: null, tagFilter: null, titleFilter: null })).toHaveLength(2);
  });
});

describe('nextUrl', () => {
  it('drops the default tab and any tag, keeps another tab', () => {
    expect(nextUrl('https://x/objects/5?tag=oil&tab=timeline', 'timeline')).toBe('/objects/5');
    expect(nextUrl('https://x/objects/5?tag=oil', 'info')).toBe('/objects/5?tab=info');
  });
});

it('foldTitle trims and lowercases', () => {
  expect(foldTitle('  Oil Change ')).toBe('oil change');
});
