import { describe, expect, it } from 'vitest';
import { previewDates } from '../src/lib/recurrence';

describe('calendar previews', () => {
  it('keeps the intended monthly day after February', () => {
    expect(previewDates('monthly:31', '2028-01-31')).toEqual(['2028-01-31', '2028-02-29', '2028-03-31']);
    expect(previewDates('monthly:last', '2027-01-31')).toEqual(['2027-01-31', '2027-02-28', '2027-03-31']);
  });
  it('supports weekly, yearly and daily boundaries', () => {
    expect(previewDates('weekly:1', '2026-09-22')).toEqual(['2026-09-28', '2026-10-05', '2026-10-12']);
    expect(previewDates('yearly:2:29', '2027-03-01')).toEqual(['2028-02-29', '2029-02-28', '2030-02-28']);
    expect(previewDates('daily', '2026-12-31')).toEqual(['2026-12-31', '2027-01-01', '2027-01-02']);
  });
});
