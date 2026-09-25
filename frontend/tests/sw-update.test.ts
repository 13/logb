import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { applyUpdate, dismissUpdate, HOUR, needReload, offerUpdate, resetUpdateForTesting, scheduleUpdateChecks, updateReady } from '../src/lib/sw-update';

beforeEach(() => resetUpdateForTesting());

describe('a waiting service worker', () => {
  it('is offered, never applied on its own', () => {
    const apply = vi.fn(async () => {});
    offerUpdate(apply);
    expect(get(updateReady)).toBe(true);
    expect(apply).not.toHaveBeenCalled();
  });

  it('is applied when the user says so', async () => {
    const apply = vi.fn(async () => {});
    offerUpdate(apply);
    await applyUpdate();
    expect(apply).toHaveBeenCalledOnce();
    expect(get(updateReady)).toBe(false);
  });

  it('can be put off', () => {
    offerUpdate(async () => {});
    dismissUpdate();
    expect(get(updateReady)).toBe(false);
  });
});

describe('a new worker taking control', () => {
  it('reloads the tab that asked for it', async () => {
    const reload = vi.fn();
    offerUpdate(async () => { needReload(reload); });
    await applyUpdate();
    expect(reload).toHaveBeenCalledOnce();
  });

  it('only offers a reload in a tab that did not ask -- another tab accepted the update', async () => {
    const reload = vi.fn();
    needReload(reload);
    expect(reload).not.toHaveBeenCalled();
    expect(get(updateReady)).toBe(true);
    await applyUpdate();
    expect(reload).toHaveBeenCalledOnce();
  });
});

describe('scheduleUpdateChecks', () => {
  it('asks the registration for a new worker every hour, while online', () => {
    let tick: () => void = () => {};
    const timer = vi.fn((fn: () => void, ms: number) => { tick = fn; expect(ms).toBe(HOUR); return 1; });
    const update = vi.fn(async () => {});
    let online = true;
    scheduleUpdateChecks({ update }, timer, () => online);
    expect(timer).toHaveBeenCalledOnce();
    tick();
    expect(update).toHaveBeenCalledOnce();
    online = false;
    tick();
    expect(update).toHaveBeenCalledOnce();
  });

  it('swallows a failed check', async () => {
    let tick: () => void = () => {};
    const update = vi.fn(async () => { throw new Error('offline'); });
    scheduleUpdateChecks({ update }, (fn) => { tick = fn; return 1; }, () => true);
    expect(() => tick()).not.toThrow();
    await Promise.resolve();
  });
});
