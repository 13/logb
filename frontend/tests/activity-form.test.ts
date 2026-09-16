import { describe, it, expect } from 'vitest';
import { activityTitle, emptyActivity, toActivityInput, validateActivity, groupByYear, exifDate, suggestionsFor } from '../src/lib/activity-form';
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
    });
  });

  it('maps an activity to input, trip fields included', () => {
    const src = { ...a(1, '2024-01-01'), cost_cents: 500, counter_value: 12, quantity_milli: 41_300, tags: ['Winter'] };
    expect(toActivityInput(src)).toEqual({
      date: '2024-01-01', category: 'repair', title: 't1', notes: '', counter_value: 12, cost_cents: 500, quantity_milli: 41_300, tags: ['Winter'],
      start_counter: null, from_place: null, to_place: null, duration_minutes: null, battery_used_pct: null,
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
});
