import { describe, expect, it } from 'vitest';
import { parseResourceCsv } from '../src/lib/resource-csv';
import { errorMessage, I18nError } from '../src/lib/api-error';
import en from '../src/i18n/en';

describe('resource CSV import', () => {
  it('parses cumulative meter readings and decimal costs', () => {
    const rows = parseResourceCsv('date;reading;cost;estimated\n2026-09-20;123,456;12,34;yes', 'meter');
    expect(rows[0]).toMatchObject({ date: '2026-09-20', meter_reading_milli: 123456, quantity_milli: null, cost_cents: 1234, estimated: 1 });
  });

  it('says what is wrong as an i18n key, never an English sentence', () => {
    const caught = (text: string, mode: 'meter' | 'usage') => {
      try { parseResourceCsv(text, mode); } catch (e) { return e as I18nError; }
      throw new Error('did not throw');
    };
    expect(caught('date;reading', 'meter')).toMatchObject({ message: 'resource.csv-too-short' });
    expect(caught('date;cost\n2026-09-20;1', 'meter')).toMatchObject({ message: 'resource.csv-needs-reading' });
    expect(caught('date;cost\n2026-09-20;1', 'usage')).toMatchObject({ message: 'resource.csv-needs-amount' });
    const date = caught('date,amount\n2026-09-20,1\n20.09.2026,1', 'usage');
    expect(date).toBeInstanceOf(I18nError);
    expect(date).toMatchObject({ message: 'resource.csv-bad-date', vars: { line: 3 } });
    expect(caught('date,amount\n2026-09-20,-1', 'usage')).toMatchObject({ message: 'resource.csv-bad-value', vars: { line: 2 } });
    for (const key of ['resource.csv-too-short', 'resource.csv-needs-reading', 'resource.csv-needs-amount', 'resource.csv-bad-date', 'resource.csv-bad-value']) {
      expect(en, key).toHaveProperty([key]);
    }
    expect(errorMessage(date, (k, v) => (en as Record<string, string>)[k].replace('{line}', String(v?.line)))).toBe('Line 3: the date must look like 2026-09-20.');
  });

  it('parses direct usage periods', () => {
    const rows = parseResourceCsv('date,amount,period_start,period_end\n2026-09-20,12.5,2026-09-01,2026-09-20', 'usage');
    expect(rows[0]).toMatchObject({ quantity_milli: 12500, period_start: '2026-09-01', period_end: '2026-09-20' });
  });
});
