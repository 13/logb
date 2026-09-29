import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { debouncer } from '../src/lib/debounce';

describe('debouncer', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it('applies only the last value of a burst, once the burst has paused', () => {
    const applied: string[] = [];
    const d = debouncer<string>((v) => applied.push(v), 100);
    d.push('b'); vi.advanceTimersByTime(50);
    d.push('bo'); vi.advanceTimersByTime(50);
    d.push('boi'); vi.advanceTimersByTime(99);
    expect(applied).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(applied).toEqual(['boi']);
  });

  it('applies an immediate value at once and drops the one still waiting', () => {
    const applied: string[] = [];
    const d = debouncer<string>((v) => applied.push(v), 100, (v) => v.trim() === '');
    d.push('boil');
    d.push('  ');
    expect(applied).toEqual(['  ']);
    vi.advanceTimersByTime(500);
    expect(applied).toEqual(['  ']);
  });

  it('applies nothing once cancelled', () => {
    const applied: string[] = [];
    const d = debouncer<string>((v) => applied.push(v), 100);
    d.push('x');
    d.cancel();
    vi.advanceTimersByTime(500);
    expect(applied).toEqual([]);
  });
});
