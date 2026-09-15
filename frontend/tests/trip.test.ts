import { describe, expect, it } from 'vitest';
import { formatDuration, parseDuration, placesLabel, spanLabel, tripDistance } from '../src/lib/trip';

describe('parseDuration', () => {
  it('reads h:mm', () => {
    expect(parseDuration('1:15')).toBe(75);
  });

  it('reads plain minutes', () => {
    expect(parseDuration('75')).toBe(75);
  });

  it('treats empty input as nothing entered', () => {
    expect(parseDuration('')).toBeNull();
  });

  it('rejects a minute part past 59', () => {
    expect(parseDuration('1:75')).toBeNaN();
  });

  it('rejects zero -- a trip has to take at least a minute', () => {
    expect(parseDuration('0')).toBeNaN();
  });

  it('rejects text that is neither h:mm nor a number', () => {
    expect(parseDuration('abc')).toBeNaN();
  });

  it('rejects a duration past the backend\'s 10080-minute (one week) cap', () => {
    expect(parseDuration('10081')).toBeNaN();
  });
});

describe('formatDuration', () => {
  it('renders h:mm, zero-padding the minutes', () => {
    expect(formatDuration(75)).toBe('1:15');
    expect(formatDuration(5)).toBe('0:05');
    expect(formatDuration(600)).toBe('10:00');
  });
});

describe('tripDistance', () => {
  it('is end minus start', () => {
    expect(tripDistance({ start_counter: 400, counter_value: 600 })).toBe(200);
  });

  it('is null with no start', () => {
    expect(tripDistance({ start_counter: null, counter_value: 600 })).toBeNull();
  });

  it('is null with no end', () => {
    expect(tripDistance({ start_counter: 400, counter_value: null })).toBeNull();
  });
});

describe('placesLabel', () => {
  it('joins both places with an arrow', () => {
    expect(placesLabel('Home', 'Office')).toBe('Home → Office');
  });

  it('shows a trailing arrow with only a departure', () => {
    expect(placesLabel('Home', null)).toBe('Home →');
  });

  it('shows a leading arrow with only a destination', () => {
    expect(placesLabel(undefined, 'Office')).toBe('→ Office');
  });

  it('is empty with neither place set', () => {
    expect(placesLabel(null, undefined)).toBe('');
    expect(placesLabel('  ', '')).toBe('');
  });
});

describe('spanLabel', () => {
  it('joins two already-formatted readings with the trip arrow', () => {
    expect(spanLabel('400 km', '600 km')).toBe('400 km → 600 km');
  });
});
