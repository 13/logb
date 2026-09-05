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
