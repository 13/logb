import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { clearToast, toast, toastMessage } from '../src/lib/toast';

describe('toast', () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it('shows one message and clears it after its time', () => {
    toast('Saved', 1000);
    expect(get(toastMessage)?.text).toBe('Saved');
    vi.advanceTimersByTime(999);
    expect(get(toastMessage)?.text).toBe('Saved');
    vi.advanceTimersByTime(1);
    expect(get(toastMessage)).toBeNull();
  });

  it('a newer message replaces the older one and keeps its own time', () => {
    toast('A', 1000);
    vi.advanceTimersByTime(800);
    toast('B', 1000);
    vi.advanceTimersByTime(300);
    expect(get(toastMessage)?.text).toBe('B');
    // B was shown at 800 ms for 1000 ms: still there at 1799, gone at 1800.
    vi.advanceTimersByTime(699);
    expect(get(toastMessage)?.text).toBe('B');
    vi.advanceTimersByTime(1);
    expect(get(toastMessage)).toBeNull();
  });

  // The Toaster calls this when its page goes, so "Saved" never shows up on the next page.
  it('clearToast removes the message and its timer', () => {
    toast('Saved', 1000);
    clearToast();
    expect(get(toastMessage)).toBeNull();
    toast('Next', 5000);
    vi.advanceTimersByTime(1000);
    expect(get(toastMessage)?.text).toBe('Next');
  });

  it('the same text twice is two messages, so it is announced twice', () => {
    toast('Saved');
    const first = get(toastMessage)?.id;
    toast('Saved');
    expect(get(toastMessage)?.id).not.toBe(first);
  });
});
