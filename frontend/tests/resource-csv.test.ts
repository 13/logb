import { describe, expect, it } from 'vitest';
import { parseResourceCsv } from '../src/lib/resource-csv';

describe('resource CSV import', () => {
  it('parses cumulative meter readings and decimal costs', () => {
    const rows = parseResourceCsv('date;reading;cost;estimated\n2026-09-20;123,456;12,34;yes', 'meter');
    expect(rows[0]).toMatchObject({ date: '2026-09-20', meter_reading_milli: 123456, quantity_milli: null, cost_cents: 1234, estimated: 1 });
  });

  it('parses direct usage periods', () => {
    const rows = parseResourceCsv('date,amount,period_start,period_end\n2026-09-20,12.5,2026-09-01,2026-09-20', 'usage');
    expect(rows[0]).toMatchObject({ quantity_milli: 12500, period_start: '2026-09-01', period_end: '2026-09-20' });
  });
});
