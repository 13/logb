import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { createQueued, ensureOutboxCounts, flushOutbox, onOutboxFlushed, outboxCounts, refreshOutboxCounts, setOutboxStoreForTesting } from '../src/lib/api-outbox';
import { setOutboxSendGate, setOutboxUser } from '../src/lib/api';
import { memoryStore, type OutboxStore, type QueuedOp } from '../src/lib/outbox';

function op(id: string, extra: Partial<QueuedOp> = {}): QueuedOp {
  return { id, kind: 'activity.create', path: '/objects/1/activities', body: {}, attempts: 0, userId: 1, ...extra };
}

/** A memory store that records how often the whole queue was read. */
function spiedStore(): OutboxStore & { reads: number } {
  const inner = memoryStore();
  const s = { ...inner, reads: 0, async all() { s.reads++; return inner.all(); } };
  return s;
}

const settle = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  setOutboxUser(1);
  setOutboxSendGate(() => true);
});

describe('outbox counts', () => {
  it('follows every write to the queue without anyone asking', async () => {
    const store = spiedStore();
    setOutboxStoreForTesting(store);
    await refreshOutboxCounts();
    expect(get(outboxCounts)).toEqual({ pending: 0, dead: 0 });

    globalThis.fetch = vi.fn(async () => { throw new TypeError('Failed to fetch'); }) as unknown as typeof fetch;
    await createQueued('/objects/1/activities', { title: 'x' }, -1);
    await settle();
    expect(get(outboxCounts)).toEqual({ pending: 1, dead: 0 });

    // Written behind the module's back (another tab, say): seen on the next recount.
    await store.put(op('b', { dead: true }));
    await store.put(op('c', { userId: 2 })); // someone else's: not counted
    await refreshOutboxCounts();
    expect(get(outboxCounts)).toEqual({ pending: 1, dead: 1 });
  });

  it('reads nothing for an empty queue, and a mount does not recount what is known', async () => {
    const store = spiedStore();
    setOutboxStoreForTesting(store);
    await refreshOutboxCounts();
    ensureOutboxCounts();
    ensureOutboxCounts();
    await settle();
    expect(store.reads).toBe(0);
  });

  it('recounts on a mount once the signed-in user changed', async () => {
    const store = spiedStore();
    setOutboxStoreForTesting(store);
    await store.put(op('a', { userId: 2 }));
    await refreshOutboxCounts();
    expect(get(outboxCounts).pending).toBe(0);
    setOutboxUser(2);
    ensureOutboxCounts();
    await settle();
    expect(get(outboxCounts).pending).toBe(1);
  });

  it('a flush over an empty queue returns at once, reading nothing and reporting no change', async () => {
    const store = spiedStore();
    setOutboxStoreForTesting(store);
    await refreshOutboxCounts();
    const heard: boolean[] = [];
    const off = onOutboxFlushed((_r, changed) => heard.push(changed));
    await flushOutbox();
    off();
    expect(heard).toEqual([false]);
    expect(store.reads).toBe(0);
  });
});
