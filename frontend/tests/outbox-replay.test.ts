import { describe, it, expect, beforeEach, vi } from 'vitest';
import { createQueued, flushOutbox, setOutboxStoreForTesting } from '../src/lib/api';
import { memoryStore } from '../src/lib/outbox';

/**
 * The offline outbox's whole no-duplicate guarantee rests on one property: the `client_op_id`
 * sent when a write is first attempted is the SAME id sent when it is later replayed (see the
 * comment on `createQueued` in `../src/lib/api.ts`). `createQueued` and `flushOutbox` have no
 * unit coverage of their own -- only `isRejection`, which they depend on, is covered here -- so
 * this property was pinned only by the (much slower, much less precise) Playwright run. This
 * test drives both functions directly, against a `memoryStore()` this test controls, and would
 * fail immediately if a future change ever minted a fresh id for the replay.
 */
describe('createQueued + flushOutbox', () => {
  beforeEach(() => {
    setOutboxStoreForTesting(memoryStore());
  });

  it('replays a queued write with the exact client_op_id the first attempt sent', async () => {
    const calls: Array<Record<string, unknown>> = [];
    let attempt = 0;
    globalThis.fetch = vi.fn(async (_url: string, init?: RequestInit) => {
      attempt++;
      calls.push(JSON.parse(init?.body as string));
      if (attempt === 1) {
        // A dropped connection: fetch itself never gets an answer to reject or resolve with,
        // which is exactly what `isRejection` treats as "unknown, so queue it" rather than a
        // server refusal.
        throw new TypeError('Failed to fetch');
      }
      return {
        ok: true,
        status: 201,
        headers: { get: (k: string) => (k.toLowerCase() === 'content-type' ? 'application/json' : null) },
        json: async () => ({ id: 42 }),
        text: async () => '{"id":42}',
      } as unknown as Response;
    }) as unknown as typeof fetch;

    const result = await createQueued('/objects/1/activities', { date: '2024-01-01', category: 'fuel', title: 'x' });
    expect(result).toBeNull(); // queued, not sent successfully

    expect(calls).toHaveLength(1);
    const firstOpId = calls[0].client_op_id;
    expect(typeof firstOpId).toBe('string');

    await flushOutbox();

    expect(calls).toHaveLength(2);
    const replayedOpId = calls[1].client_op_id;
    expect(replayedOpId).toBe(firstOpId);
  });
});
