import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { autosave } from '../src/lib/autosave';

/** Every send asks for keepalive, so one still out when the page is left is not cancelled. */
const K = { keepalive: true };

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
    expect(save.mock.calls).toEqual([[3, K]]);
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
    expect(save.mock.calls).toEqual([[1, K]]);
    release();
    await vi.advanceTimersByTimeAsync(0);
    expect(save.mock.calls).toEqual([[1, K], [3, K]]);
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
    expect(save.mock.calls).toEqual([['x', K]]);
  });

  it('does not report an older value failing when a newer one is on its way, and says Saved for the newer', async () => {
    let reject!: (e: unknown) => void;
    const save = vi.fn((v: number) => (v === 1 ? new Promise<void>((_, r) => { reject = r; }) : Promise.resolve()));
    const onsaved = vi.fn();
    const onerror = vi.fn();
    const s = autosave(save, { delay: 0, onsaved, onerror });
    s.push(1);
    await vi.advanceTimersByTimeAsync(0);
    s.push(2);
    reject(new Error('refused'));
    await vi.advanceTimersByTimeAsync(0);
    expect(save.mock.calls).toEqual([[1, K], [2, K]]);
    expect(onerror).not.toHaveBeenCalled();
    expect(onsaved).toHaveBeenCalledTimes(1);
  });

  it('keeps working after a failure: the next change is sent and says Saved', async () => {
    const save = vi.fn(async (v: number) => { if (v === 1) throw new Error('refused'); });
    const onsaved = vi.fn();
    const onerror = vi.fn();
    const s = autosave(save, { delay: 0, onsaved, onerror });
    s.push(1);
    await vi.advanceTimersByTimeAsync(0);
    expect(onerror).toHaveBeenCalledTimes(1);
    s.push(2);
    await vi.advanceTimersByTimeAsync(0);
    expect(save.mock.calls).toEqual([[1, K], [2, K]]);
    expect(onsaved).toHaveBeenCalledTimes(1);
  });

  it('retry sends the current value again, without a new change', async () => {
    let fail = true;
    const save = vi.fn(async (_v: string) => { if (fail) throw new Error('offline'); });
    const onsaved = vi.fn();
    const onerror = vi.fn();
    const s = autosave(save, { delay: 300, onsaved, onerror });
    s.push('a');
    await vi.advanceTimersByTimeAsync(300);
    expect(onerror).toHaveBeenCalledTimes(1);
    fail = false;
    s.retry();
    await vi.advanceTimersByTimeAsync(0);
    expect(save.mock.calls).toEqual([['a', K], ['a', K]]);
    expect(onsaved).toHaveBeenCalledTimes(1);
  });

  it('flush while a request is out resolves only once the waiting value has been sent too', async () => {
    let release!: () => void;
    const save = vi.fn((v: number) => (v === 1 ? new Promise<void>((r) => { release = r; }) : Promise.resolve()));
    const s = autosave(save, { delay: 10_000 });
    s.push(1);
    await vi.advanceTimersByTimeAsync(10_000);
    s.push(2);
    let done = false;
    const flushed = s.flush().then(() => { done = true; });
    await vi.advanceTimersByTimeAsync(0);
    expect(done).toBe(false);
    release();
    await flushed;
    expect(save.mock.calls).toEqual([[1, K], [2, K]]);
  });

  it('flush with nothing waiting resolves at once and sends nothing', async () => {
    const save = vi.fn(async (_v: number) => {});
    const s = autosave(save);
    await expect(s.flush()).resolves.toBeUndefined();
    expect(save).not.toHaveBeenCalled();
  });

  it('flush while a send is already running: that send asked for keepalive, so leaving does not cancel it', async () => {
    let release!: () => void;
    const save = vi.fn((_v: number, _o?: { keepalive: true }) => new Promise<void>((r) => { release = r; }));
    const onsaved = vi.fn();
    const s = autosave(save, { delay: 0, onsaved });
    s.push(1);
    await vi.advanceTimersByTimeAsync(0);
    // The request is out before the page is left: nothing is waiting, flush only waits for it.
    const flushed = s.flush();
    expect(save.mock.calls).toEqual([[1, K]]);
    release();
    await flushed;
    expect(save).toHaveBeenCalledTimes(1);
    expect(onsaved).toHaveBeenCalledTimes(1);
  });
});
