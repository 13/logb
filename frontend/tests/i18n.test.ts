import { describe, it, expect } from 'vitest';
import en from '../src/i18n/en';
import de from '../src/i18n/de';
import { pickLocale } from '../src/i18n/detect';

describe('i18n', () => {
  it('en and de have the same keys', () => {
    const a = Object.keys(en).sort();
    const b = Object.keys(de).sort();
    expect(b).toEqual(a);
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
  it('shows the fallback language until the chosen one has loaded, then the chosen one', async () => {
    const { get } = await import('svelte/store');
    const { settings } = await import('../src/stores/settings');
    const i18n = await import('../src/i18n');
    settings.update((s) => ({ ...s, locale: 'de' }));
    // Whatever `t` shows before the chunk arrives, it is never the bare key.
    expect(get(i18n.t)('nav.loading')).not.toBe('nav.loading');
    await i18n.ensureLocale('de');
    expect(get(i18n.t)('nav.loading')).toBe(de['nav.loading']);
    settings.update((s) => ({ ...s, locale: 'en' }));
    expect(get(i18n.t)('nav.loading')).toBe(en['nav.loading']);
  });
});
