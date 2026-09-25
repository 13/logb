import { describe, expect, it } from 'vitest';
import { hashToNegativeId } from '../src/lib/activity-form';
import { newOpId } from '../src/lib/outbox';

describe('temp ids', () => {
  it('mint on a plain-http origin, where crypto.randomUUID does not exist', () => {
    const original = globalThis.crypto.randomUUID;
    // @ts-expect-error -- simulating a non-secure context
    globalThis.crypto.randomUUID = undefined;
    try {
      expect(hashToNegativeId(newOpId())).toBeLessThan(0);
    } finally {
      globalThis.crypto.randomUUID = original;
    }
  });
});
