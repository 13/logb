import { describe, expect, it } from 'vitest';
import { templateInput, templatesFor } from '../src/lib/reminder-templates';
import en from '../src/i18n/en';
import de from '../src/i18n/de';

const today = '2026-09-14';

describe('reminder templates', () => {
  it('offers a car its services, and the reading only when it has a counter', () => {
    expect(templatesFor('car', 'km').map((t) => t.id)).toEqual(['oil', 'inspection', 'tyres', 'reading']);
    expect(templatesFor('car', null).map((t) => t.id)).toEqual(['oil', 'inspection', 'tyres']);
  });

  it('leaves out a template that is due by distance alone when there is no counter', () => {
    expect(templatesFor('motorcycle', null).map((t) => t.id)).not.toContain('chain');
    expect(templatesFor('motorcycle', 'km').map((t) => t.id)).toContain('chain');
  });

  it('sets the distance from the current reading, in the unit the dashboard shows', () => {
    const oil = templatesFor('car', 'mi').find((t) => t.id === 'oil')!;
    expect(templateInput(oil, { title: 'Oil', unit: 'mi', currentReading: 80_000, today })).toMatchObject({
      kind: 'service', due_date: '2027-09-14', repeat_months: 12, due_counter: 90_000, repeat_counter: 10_000,
    });
  });

  it('falls back to the date without a reading, and skips what would be created wrong', () => {
    const oil = templatesFor('car', 'km').find((t) => t.id === 'oil')!;
    expect(templateInput(oil, { title: 'Oil', unit: 'km', currentReading: null, today })).toMatchObject({
      due_date: '2027-09-14', due_counter: null, repeat_counter: null,
    });
    const chain = templatesFor('motorcycle', 'km').find((t) => t.id === 'chain')!;
    expect(templateInput(chain, { title: 'Chain', unit: 'km', currentReading: null, today })).toBeNull();
  });

  it('starts a reading reminder a month out', () => {
    const reading = templatesFor('tool', 'h').find((t) => t.id === 'reading')!;
    expect(templateInput(reading, { title: 'Log', unit: 'h', currentReading: 12, today })).toMatchObject({
      kind: 'reading', every_n: 1, every_unit: 'month', due_date: '2026-10-14',
    });
  });

  it('every template title exists in both languages', () => {
    const types = ['car', 'e_bike', 'bike', 'motorcycle', 'home', 'appliance', 'tool', 'body', 'other'] as const;
    for (const type of types) {
      for (const t of templatesFor(type, 'km')) {
        expect(en, t.title).toHaveProperty([t.title]);
        expect(de, t.title).toHaveProperty([t.title]);
      }
    }
  });
});
