import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { autosave } from '../src/lib/autosave';

describe('autosave', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it('waits for a pause and sends only the newest value', async () => {
    const save = vi.fn(async (_v: number) => {});
    const onsaved = vi.fn();
    const s = autosave(save, { delay: 300, onsaved });
    s.push(1); s.push(2); s.push(3);
    await vi.advanceTimersByTimeAsync(299);
    expect(save).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(save.mock.calls).toEqual([[3]]);
    expect(onsaved).toHaveBeenCalledTimes(1);
  });

  it('sends one request at a time, then the newest value that arrived meanwhile', async () => {
    let release!: () => void;
    const save = vi.fn((v: number) => (v === 1 ? new Promise<void>((r) => { release = r; }) : Promise.resolve()));
    const onsaved = vi.fn();
    const s = autosave(save, { delay: 0, onsaved });
    s.push(1);
    await vi.advanceTimersByTimeAsync(0);
    s.push(2); s.push(3);
    await vi.advanceTimersByTimeAsync(0);
    expect(save.mock.calls).toEqual([[1]]);
    release();
    await vi.advanceTimersByTimeAsync(0);
    expect(save.mock.calls).toEqual([[1], [3]]);
    // "Saved" once, for the value that is now on the server -- not for the one it replaced.
    expect(onsaved).toHaveBeenCalledTimes(1);
  });

  it('reports the failure of the newest value', async () => {
    const onerror = vi.fn();
    const s = autosave(async () => { throw new Error('refused'); }, { delay: 0, onerror });
    s.push(1);
    await vi.advanceTimersByTimeAsync(0);
    expect(onerror).toHaveBeenCalledWith(expect.any(Error));
  });

  it('sends a waiting value at once when flushed (leaving the page)', async () => {
    const save = vi.fn(async (_v: string) => {});
    const s = autosave(save, { delay: 10_000 });
    s.push('x');
    await s.flush();
    expect(save.mock.calls).toEqual([['x']]);
  });
});
