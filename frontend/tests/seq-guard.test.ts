import { describe, it, expect } from 'vitest';
import { createSeq } from '../src/lib/seq-guard';

describe('createSeq', () => {
  it('lets only the newest load commit', () => {
    const seq = createSeq();
    const first = seq.next();
    const second = seq.next();
    expect(seq.current(first)).toBe(false);
    expect(seq.current(second)).toBe(true);
  });

  it('invalidates every load in flight, for a navigation to another object', () => {
    const seq = createSeq();
    const token = seq.next();
    seq.invalidate();
    expect(seq.current(token)).toBe(false);
  });
});
