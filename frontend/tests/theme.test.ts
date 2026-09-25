import { describe, expect, it, vi } from 'vitest';
import { applyTheme, followTheme, isDark } from '../src/lib/theme';

describe('isDark', () => {
  it('follows the system only on auto', () => {
    expect(isDark('auto', true)).toBe(true);
    expect(isDark('auto', false)).toBe(false);
    expect(isDark('dark', false)).toBe(true);
    expect(isDark('light', true)).toBe(false);
  });
});

/** A document with a root element and the theme-color metas index.html ships. */
function fakeDoc(bg: Record<string, string>) {
  const root = { dataset: {} as Record<string, string> };
  const metas = [0, 1].map(() => {
    const attrs: Record<string, string> = { media: '(prefers-color-scheme: dark)', content: '#000000' };
    return {
      attrs,
      setAttribute: (k: string, v: string) => { attrs[k] = v; },
      removeAttribute: (k: string) => { delete attrs[k]; },
    };
  });
  const doc = {
    documentElement: root,
    querySelectorAll: (sel: string) => (sel === 'meta[name="theme-color"]' ? metas : []),
    defaultView: { getComputedStyle: () => ({ getPropertyValue: (p: string) => (p === '--bg' ? ` ${bg[root.dataset.theme]}` : '') }) },
  };
  return { doc: doc as unknown as Document, root, metas };
}

describe('applyTheme', () => {
  it("sets the theme and paints the browser's bar in the page background", () => {
    const { doc, root, metas } = fakeDoc({ dark: '#121412', light: '#f7f7f5' });
    applyTheme(doc, true);
    expect(root.dataset.theme).toBe('dark');
    for (const m of metas) expect(m.attrs).toEqual({ content: '#121412' });
    applyTheme(doc, false);
    expect(root.dataset.theme).toBe('light');
    for (const m of metas) expect(m.attrs.content).toBe('#f7f7f5');
  });
});

describe('followTheme', () => {
  function fakeMedia(matches: boolean) {
    const listeners = new Set<() => void>();
    const mq = {
      matches,
      addEventListener: vi.fn((_: string, fn: () => void) => listeners.add(fn)),
      removeEventListener: vi.fn((_: string, fn: () => void) => listeners.delete(fn)),
    };
    return { mq, change(to: boolean) { mq.matches = to; for (const fn of listeners) fn(); }, listeners };
  }

  it('on auto, follows a change of the system setting while the app is open', () => {
    const apply = vi.fn();
    const m = fakeMedia(false);
    const stop = followTheme('auto', m.mq as unknown as MediaQueryList, apply);
    expect(apply).toHaveBeenLastCalledWith(false);
    m.change(true);
    expect(apply).toHaveBeenLastCalledWith(true);
    stop();
    expect(m.listeners.size).toBe(0);
  });

  it('with a fixed theme, ignores the system entirely', () => {
    const apply = vi.fn();
    const m = fakeMedia(true);
    followTheme('light', m.mq as unknown as MediaQueryList, apply);
    expect(apply).toHaveBeenLastCalledWith(false);
    expect(m.listeners.size).toBe(0);
  });
});
