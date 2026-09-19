import { describe, expect, it } from 'vitest';
import { parseWeight, weightInput, formatWeight, orderWeights, quickLogPath } from '../src/lib/weight';
import { validateActivity, emptyActivity } from '../src/lib/activity-form';

describe('weight measurements', () => {
  it('accepts decimal commas and dots, stores grams, and rejects malformed values', () => {
    expect(parseWeight(' 72,35 ', 'kg')).toBe(72350);
    expect(parseWeight('72.35', 'kg')).toBe(72350);
    expect(parseWeight('160', 'lb')).toBe(72575);
    for (const value of ['', '-1', '0', '1e4', '1,2,3', 'NaN', 'Infinity', '1 200', '1000001']) expect(parseWeight(value, 'kg')).toBeNaN();
  });
  it('converts stored measurements without changing storage and formats neutral decreases', () => {
    expect(weightInput(72350, 'kg')).toBe('72.35');
    expect(formatWeight(72350, 'kg', 'de')).toBe('72,35 kg');
    expect(formatWeight(-1500, 'kg', 'en')).toBe('-1.5 kg');
    expect(parseWeight(weightInput(72350, 'lb'), 'lb')).toBe(72350);
  });
  it('orders by measurement date, creation date and ID, never by largest value', () => {
    const points = [
      { id: 1, date: '2026-01-01', created_at: '2026-02-01', weight_grams: 90000 },
      { id: 3, date: '2026-01-02', created_at: '2026-01-02', weight_grams: 80000 },
      { id: 2, date: '2026-01-02', created_at: '2026-01-02', weight_grams: 85000 },
    ];
    expect(orderWeights(points).map(p => p.id)).toEqual([3, 2, 1]);
    expect(points[0].id).toBe(1);
  });
  it('requires positive integral grams but permits a title-free weight form', () => {
    const body = { ...emptyActivity(), category: 'weight' as const, weight_grams: 72000 };
    expect(validateActivity(body)).toBeNull();
    for (const weight_grams of [null, undefined, 0, -1, 70000.1, NaN]) expect(validateActivity({ ...body, weight_grams })).toBe('weight.invalid');
  });
  it('chooses quick actions from object capabilities', () => {
    const object = {id: 1, type: 'body' as const, counter_unit: null, fuel_unit: null};
    expect(quickLogPath(object)).toContain('category=weight');
    expect(quickLogPath({...object, type: 'car', fuel_unit: 'l'})).toContain('category=fuel');
    expect(quickLogPath({...object, type: 'bike', counter_unit: 'km'})).toBe('/objects/1/reading');
    expect(quickLogPath({...object, type: 'home'})).toBe('/objects/1/activities/new');
  });
});

describe('offline weight history', () => {
  it('overlays a queued correction and creation without duplicating the stored row', async () => {
    const { withPendingWeights } = await import('../src/lib/weight');
    const original = [{id:1,date:'2026-01-01',created_at:'2026-01-01T00:00:00Z',weight_grams:80000}];
    const ops = [
      {id:'edit',kind:'activity.update' as const,path:'/activities/1',body:{category:'weight',date:'2026-01-01',weight_grams:79000},attempts:0},
      {id:'new',kind:'activity.create' as const,path:'/objects/1/activities',body:{category:'weight',date:'2026-01-02',weight_grams:78000},attempts:0},
    ];
    const merged = withPendingWeights(original,ops);
    expect(merged.map(p => p.weight_grams)).toEqual([78000,79000]);
    expect(merged.every(p => p.pending)).toBe(true);
    expect(original[0].weight_grams).toBe(80000);
    expect(withPendingWeights(original,[{...ops[0],body:{category:'other'}}])).toEqual([]);
  });
});

it('orders same-day pending entries consistently across timestamp precision and queue ties', async () => {
  const { withPendingWeights } = await import('../src/lib/weight');
  const history = [{id:5,date:'2026-01-01',created_at:'2026-01-01T00:00:00Z',weight_grams:80000}];
  const ops = [79000,78000].map((grams,i) => ({id:String(i),kind:'activity.create' as const,path:'/objects/1/activities',body:{category:'weight',date:'2026-01-01',weight_grams:grams},attempts:0,queued_at:Date.parse('2026-01-01T00:00:00.123Z')}));
  expect(withPendingWeights(history,ops).map(p=>p.weight_grams)).toEqual([78000,79000,80000]);
});
