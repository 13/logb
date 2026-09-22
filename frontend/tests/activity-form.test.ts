import { describe, it, expect } from 'vitest';
import { activityTitle, emptyActivity, toActivityInput, validateActivity, groupByYear, exifDate, suggestionsFor } from '../src/lib/activity-form';
import { hashToNegativeId, resolveCategory, parseCategoryParam, counterBelowLast, weightDeviates, firstExifDate, activityToFormText, changeWeightUnitState, buildActivityInput, optimisticActivity } from '../src/lib/activity-form';
import { categoriesFor } from '../src/lib/type-registry';
import type { Activity, TitleSuggestion } from '../src/lib/types';

function a(id: number, date: string): Activity {
  return {
    id, object_id: 1, date, category: 'repair', title: `t${id}`, notes: '', counter_value: null, cost_cents: null, quantity_milli: null, created_at: '', updated_at: '', attachments: [], tags: [],
    start_counter: null, from_place: null, to_place: null, duration_minutes: null, battery_used_pct: null,
    charged_full: 0,
  };
}

describe('activity form', () => {
  it('defaults the date to today', () => {
    expect(emptyActivity().date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(emptyActivity().category).toBe('maintenance');
    expect(emptyActivity().tags).toEqual([]);
    // The five trip-only fields start out null on every entry, trip or not -- `buildInput` in
    // ActivityForm is what actually keeps them null for a non-trip save.
    expect(emptyActivity()).toMatchObject({
      start_counter: null, from_place: null, to_place: null, duration_minutes: null, battery_used_pct: null,
      meter_reading_milli: null, period_start: null, period_end: null, estimated: 0, meter_reset: 0,
    });
  });

  it('maps an activity to input, trip fields included', () => {
    const src = { ...a(1, '2024-01-01'), cost_cents: 500, counter_value: 12, quantity_milli: 41_300, tags: ['Winter'] };
    expect(toActivityInput(src)).toEqual({
      weight_grams: null, fuel_level_pct: null,
      date: '2024-01-01', category: 'repair', title: 't1', notes: '', counter_value: 12, cost_cents: 500, quantity_milli: 41_300, tags: ['Winter'],
      start_counter: null, from_place: null, to_place: null, duration_minutes: null, battery_used_pct: null,
      meter_reading_milli: null, period_start: null, period_end: null, estimated: 0, meter_reset: 0,
    });
  });

  it('validates required fields', () => {
    const base = emptyActivity();
    expect(validateActivity({ ...base, title: '' })).toBe('activity.title');
    expect(validateActivity({ ...base, title: 'x', date: '' })).toBe('activity.date');
    expect(validateActivity({ ...base, title: 'x', cost_cents: NaN })).toBe('activity.cost');
    expect(validateActivity({ ...base, title: 'x', counter_value: NaN })).toBe('activity.counter');
    expect(validateActivity({ ...base, title: 'x', quantity_milli: NaN })).toBe('activity.quantity');
    expect(validateActivity({ ...base, title: 'x' })).toBeNull();
  });

  describe('a trip', () => {
    const trip = { ...emptyActivity(), category: 'trip' as const, title: '', start_counter: 400, counter_value: 600 };

    it('is valid with only start and end', () => {
      expect(validateActivity(trip)).toBeNull();
    });

    it('is valid with an empty title -- ActivityForm defaults it to "Trip" on save', () => {
      expect(validateActivity({ ...trip, title: '' })).toBeNull();
    });

    it('rejects an end below start', () => {
      expect(validateActivity({ ...trip, counter_value: 300 })).toBe('trip.end');
    });

    it('rejects a missing start', () => {
      expect(validateActivity({ ...trip, start_counter: null })).toBe('trip.start');
    });

    it('rejects battery used past 100', () => {
      expect(validateActivity({ ...trip, battery_used_pct: 101 })).toBe('trip.battery');
    });

    it('rejects a duration that failed to parse', () => {
      expect(validateActivity({ ...trip, duration_minutes: NaN })).toBe('trip.duration');
    });
  });

  it('still requires a title on a non-trip, non-fuel entry with an empty one', () => {
    expect(validateActivity({ ...emptyActivity(), category: 'maintenance', title: '' })).toBe('activity.title');
  });

  it('a fuel entry is valid with an empty title too -- ActivityForm defaults it, like a trip', () => {
    expect(validateActivity({ ...emptyActivity(), category: 'fuel', title: '' })).toBeNull();
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

describe('activityTitle', () => {
  const strings: Record<string, string> = { 'cat.trip': 'Trip', 'energy.charged-total': 'Charged', 'insights.fuel-total': 'Fuel logged' };
  const t = (key: string) => strings[key] ?? key;

  it('is the title itself when there is one', () => {
    expect(activityTitle('Repaint', 'repair', t)).toBe('Repaint');
  });

  it('falls back to "Trip" for an untitled trip', () => {
    expect(activityTitle('', 'trip', t)).toBe('Trip');
  });

  it('falls back to the fill wording for an untitled charge with no fuel unit, or a non-kWh one', () => {
    expect(activityTitle('', 'fuel', t)).toBe('Fuel logged');
    expect(activityTitle('', 'fuel', t, 'l')).toBe('Fuel logged');
    expect(activityTitle('', 'fuel', t, 'gal')).toBe('Fuel logged');
  });

  it('falls back to "Charged" for an untitled charge on a kWh object', () => {
    expect(activityTitle('', 'fuel', t, 'kwh')).toBe('Charged');
  });

  it('stays empty for any other category -- only a trip or a charge ever reaches here blank', () => {
    expect(activityTitle('', 'maintenance', t)).toBe('');
  });

  it('reads the same generic wording a caller with no object context (a search hit) gets, since fuelUnit is optional', () => {
    expect(activityTitle('', 'fuel', t)).toBe(activityTitle('', 'fuel', t, undefined));
  });
});

const s = (title: string, category: string): TitleSuggestion =>
  ({ title, category, last_date: '2026-01-01', last_cost_cents: null, last_counter: null }) as TitleSuggestion;

describe('suggestionsFor', () => {
  it('narrows to the chosen category', () => {
    const all = [s('Fuel', 'fuel'), s('Oil change', 'maintenance')];
    expect(suggestionsFor(all, 'fuel').map((x) => x.title)).toEqual(['Fuel']);
  });

  it('keeps order and drops repeated titles when no category is chosen', () => {
    const all = [s('Fuel', 'fuel'), s('Fuel', 'other'), s('Oil change', 'maintenance')];
    expect(suggestionsFor(all, null).map((x) => x.title)).toEqual(['Fuel', 'Oil change']);
  });

  it('returns an empty list when nothing matches', () => {
    expect(suggestionsFor([s('Fuel', 'fuel')], 'repair')).toEqual([]);
  });

  // An untitled trip's own recent-title entry has nothing worth repeating -- a "Repeat: " chip
  // with nothing after the colon -- so it never reaches the list at all.
  it('drops an untitled trip', () => {
    const all = [s('', 'trip'), s('Commute', 'trip')];
    expect(suggestionsFor(all, 'trip').map((x) => x.title)).toEqual(['Commute']);
  });
});

describe('categoriesFor offering trip', () => {
  it('offers trip on a distance-counter object', () => {
    expect(categoriesFor('e_bike', [], undefined, 'km')).toContain('trip');
  });

  it('does not offer trip on a non-distance counter', () => {
    expect(categoriesFor('e_bike', [], undefined, 'h')).not.toContain('trip');
  });

  it('offers fuel by resource capability, independent of object type', () => {
    expect(categoriesFor('home', [], undefined, 'h', 'l')).toContain('fuel');
    expect(categoriesFor('appliance', [], undefined, 'h', 'kwh')).toContain('fuel');
  });
});

describe('hashToNegativeId', () => {
  it('is negative, stable and never zero', () => {
    expect(hashToNegativeId('op-1')).toBeLessThan(0);
    expect(hashToNegativeId('op-1')).toBe(hashToNegativeId('op-1'));
    expect(hashToNegativeId('')).toBe(-1);
  });
});

describe('resolveCategory', () => {
  it('takes a wanted category the object offers and marks it touched', () => {
    expect(resolveCategory(['trip', 'fuel'], 'trip', 'maintenance')).toEqual({ category: 'trip', touched: true });
  });
  it('falls back to the first offered category when the current one is not offered', () => {
    expect(resolveCategory(['weight'], null, 'maintenance')).toEqual({ category: 'weight', touched: false });
  });
  it('keeps a current category that is offered', () => {
    expect(resolveCategory(['repair', 'maintenance'], null, 'maintenance')).toEqual({ category: 'maintenance', touched: false });
  });
  it('ignores a wanted category the object does not offer', () => {
    expect(resolveCategory(['repair'], 'trip', 'repair')).toEqual({ category: 'repair', touched: false });
  });
});

describe('parseCategoryParam', () => {
  it('reads a known category and ignores anything else', () => {
    expect(parseCategoryParam('?category=trip')).toBe('trip');
    expect(parseCategoryParam('?category=nope')).toBeNull();
    expect(parseCategoryParam('')).toBeNull();
  });
});

describe('warnings', () => {
  it('flags a counter below the last known one', () => {
    expect(counterBelowLast('100', 200)).toBe(true);
    expect(counterBelowLast('300', 200)).toBe(false);
    expect(counterBelowLast('', 200)).toBe(false);
    expect(counterBelowLast('100', null)).toBe(false);
  });
  it('flags a weight more than ten percent off the latest', () => {
    expect(weightDeviates('weight', '90', 'kg', 80_000)).toBe(true);
    expect(weightDeviates('weight', '81', 'kg', 80_000)).toBe(false);
    expect(weightDeviates('repair', '90', 'kg', 80_000)).toBe(false);
    expect(weightDeviates('weight', '90', 'kg', null)).toBe(false);
  });
  it('picks the first attachment with an EXIF date', () => {
    expect(firstExifDate([{ taken_at: null }, { taken_at: '2026-01-02T10:00:00Z' }])).toBe('2026-01-02');
    expect(firstExifDate([])).toBeNull();
  });
});

describe('activityToFormText', () => {
  it('renders every nullable field as text', () => {
    const t = activityToFormText({ ...a(1, '2026-01-01'), category: 'trip', cost_cents: 1234, counter_value: 500, quantity_milli: 42_500, meter_reading_milli: 1_500, charged_full: 1, from_place: 'A', to_place: 'B', duration_minutes: 90, weight_grams: 80_000, start_counter: 400 }, 'kg');
    expect(t).toMatchObject({ costText: '12.34', counterText: '500', quantityText: '42.5', meterReadingText: '1.5', chargedFull: true, fromText: 'A', toText: 'B', durationText: '1:30', distance: 100 });
    expect(t.weightText).toBe('80');
  });
  it('renders nulls as empty strings', () => {
    const t = activityToFormText(a(1, '2026-01-01'), 'kg');
    expect(t).toMatchObject({ costText: '', counterText: '', quantityText: '', meterReadingText: '', chargedFull: false, fromText: '', toText: '', durationText: '', distance: null });
  });
});

describe('changeWeightUnitState', () => {
  it('converts the typed text and remembers the original grams', () => {
    const s = changeWeightUnitState({ text: '80', unit: 'kg', originalText: '', originalUnit: 'kg', original: null }, 'lb');
    expect(s.unit).toBe('lb');
    expect(s.original).toBe(80_000);
    expect(Number(s.text)).toBeCloseTo(176.4, 1);
  });
  it('reuses the original grams when the text is untouched', () => {
    const s = changeWeightUnitState({ text: '176.4', unit: 'lb', originalText: '176.4', originalUnit: 'lb', original: 80_000 }, 'kg');
    expect(s.original).toBe(80_000);
    expect(s.text).toBe('80');
  });
  it('only switches the unit when the text is not a number', () => {
    const s = changeWeightUnitState({ text: 'abc', unit: 'kg', originalText: '', originalUnit: 'kg', original: null }, 'lb');
    expect(s).toEqual({ text: 'abc', unit: 'lb', originalText: '', originalUnit: 'kg', original: null });
  });
});

const texts = { weightText: '', costText: '', counterText: '', quantityText: '', meterReadingText: '', fromText: '', toText: '', durationText: '', chargedFull: true };
const weight = { text: '', unit: 'kg' as const, originalText: '', originalUnit: 'kg' as const, original: null };
const t = (k: string) => k;

describe('buildActivityInput', () => {
  it('nulls every trip field on a non-trip entry', () => {
    const out = buildActivityInput({ ...emptyActivity(), category: 'repair', start_counter: 5, battery_used_pct: 3 }, { ...texts, fromText: 'A', toText: 'B', durationText: '1:00', counterText: '700' }, weight, null, t);
    expect(out).toMatchObject({ start_counter: null, from_place: null, to_place: null, duration_minutes: null, battery_used_pct: null, counter_value: 700 });
  });
  it('keeps trip fields on a trip and reads the end from the input', () => {
    const out = buildActivityInput({ ...emptyActivity(), category: 'trip', start_counter: 400, counter_value: 600 }, { ...texts, fromText: ' A ', toText: '', durationText: '0:45' }, weight, null, t);
    expect(out).toMatchObject({ start_counter: 400, counter_value: 600, from_place: 'A', to_place: null, duration_minutes: 45 });
  });
  it('sends fuel quantity and charged_full only for fuel', () => {
    const fuel = buildActivityInput({ ...emptyActivity(), category: 'fuel' }, { ...texts, quantityText: '42,5' }, weight, null, t);
    expect(fuel).toMatchObject({ quantity_milli: 42_500, charged_full: 1 });
    const repair = buildActivityInput({ ...emptyActivity(), category: 'repair' }, { ...texts, quantityText: '42,5' }, weight, null, t);
    expect(repair).toMatchObject({ quantity_milli: null, charged_full: 0 });
  });
  it('a weight entry gets grams and the category word as its title', () => {
    const out = buildActivityInput({ ...emptyActivity(), category: 'weight', title: '' }, { ...texts, weightText: '80', costText: '5' }, weight, null, t);
    expect(out).toMatchObject({ weight_grams: 80_000, title: 'cat.weight', cost_cents: null, counter_value: null });
  });
  it('copies tags into a plain array', () => {
    const tags = ['a'];
    const out = buildActivityInput({ ...emptyActivity(), tags }, texts, weight, null, t);
    expect(out.tags).toEqual(['a']);
    expect(out.tags).not.toBe(tags);
  });
});

describe('optimisticActivity', () => {
  it('fills every nullable field and marks the row pending', () => {
    const out = optimisticActivity(-7, 3, { ...emptyActivity(), title: 'x' });
    expect(out).toMatchObject({ id: -7, object_id: 3, title: 'x', pending: true, attachments: [], charged_full: 0, estimated: 0, meter_reset: 0, tags: [] });
    expect(typeof out.created_at).toBe('string');
  });
});
