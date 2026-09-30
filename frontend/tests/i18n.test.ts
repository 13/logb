import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest';
import en from '../src/i18n/en';
import de from '../src/i18n/de';
import { pickLocale } from '../src/i18n/detect';

describe('i18n', () => {
  it('en and de have the same keys', () => {
    const a = Object.keys(en).sort();
    const b = Object.keys(de).sort();
    expect(b).toEqual(a);
  });
  // With no dictionary in the entry chunk, a key the active language lacks has no English to fall
  // back on (only the key itself), so these two tests are what the fallback used to be.
  it('every key has the same {placeholders} in en and de', () => {
    const names = (s: string) => [...new Set(s.match(/\{[^{}]+\}/g) ?? [])].sort();
    const ed: Record<string, string> = de;
    for (const [k, v] of Object.entries(en)) expect(names(ed[k] ?? ''), k).toEqual(names(v));
  });
  it('no empty translations', () => {
    for (const d of [en, de]) for (const [k, v] of Object.entries(d)) expect(v, k).not.toBe('');
  });
  it('picks a supported locale', () => {
    expect(pickLocale(['de-AT', 'en'])).toBe('de');
    expect(pickLocale(['fr-FR'])).toBe('en');
  });
});

describe('the active locale, loaded on demand', () => {
  beforeEach(() => { vi.resetModules(); });
  afterEach(() => { vi.doUnmock('../src/i18n/de'); vi.doUnmock('../src/i18n/en'); vi.restoreAllMocks(); });

  async function fresh(pref: 'en' | 'de') {
    const { get } = await import('svelte/store');
    const { settings } = await import('../src/stores/settings');
    settings.update((s) => ({ ...s, locale: pref }));
    const i18n = await import('../src/i18n');
    return { get, settings, i18n, tr: (k: string) => get(i18n.t)(k) };
  }

  it('shows the chosen language once it has loaded, and no dictionary is in hand before that', async () => {
    const { settings, i18n, tr } = await fresh('de');
    // Nothing is bundled: before the first load finishes (main.ts waits for it) only keys exist.
    expect(tr('nav.loading')).toBe('nav.loading');
    await i18n.ensureLocale('de');
    expect(tr('nav.loading')).toBe(de['nav.loading']);
    // A switch keeps the language on screen until the new one arrives, never bare keys.
    settings.update((s) => ({ ...s, locale: 'en' }));
    expect(tr('nav.loading')).toBe(de['nav.loading']);
    await i18n.ensureLocale('en');
    expect(tr('nav.loading')).toBe(en['nav.loading']);
  });

  it('English, when first chosen, is loaded the same way', async () => {
    const { i18n, tr } = await fresh('en');
    await i18n.ensureLocale('en');
    expect(tr('nav.loading')).toBe(en['nav.loading']);
  });

  it('falls back to English when the chosen language cannot be loaded, and does not remember the failure', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.doMock('../src/i18n/de', () => { throw new Error('offline'); });
    const { i18n, tr } = await fresh('de');
    await i18n.ensureLocale('de');
    expect(tr('nav.loading')).toBe(en['nav.loading']);
    // Not remembered by the i18n module (the browser may still remember a failed import).
    vi.doUnmock('../src/i18n/de');
    await i18n.ensureLocale('de');
    expect(tr('nav.loading')).toBe(de['nav.loading']);
  });

  it('falls back to another language when English itself cannot be loaded', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.doMock('../src/i18n/en', () => { throw new Error('offline'); });
    const { i18n, tr } = await fresh('en');
    await i18n.ensureLocale('en');
    expect(tr('nav.loading')).toBe(de['nav.loading']);
  });
});
