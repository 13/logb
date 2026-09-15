import { derived, writable } from 'svelte/store';
import { settings } from '../stores/settings';
import { detectLocale, isLocale, navigatorTags, FALLBACK, type Locale } from './detect';
import en from './en';
import de from './de';

const dicts: Record<Locale, Record<string, string>> = { en, de };

export const browserLang = writable<Locale>(detectLocale());
/** The full `navigator.languages` tag list (region included), kept live. `browserLang` alone is
 *  not enough for anything that reads the REGION (see `resolveDateFormat`): its own primary
 *  subtag can stay `en` across an `en-GB` → `en-US` change, so a store that only updated on a
 *  primary-language change would silently miss a region-only one. */
export const navigatorLangs = writable<readonly string[]>(navigatorTags(globalThis.navigator));
globalThis.addEventListener?.('languagechange', () => {
  browserLang.set(detectLocale());
  navigatorLangs.set(navigatorTags(globalThis.navigator));
});

export const locale = derived([settings, browserLang], ([$s, $b]): Locale => {
  const want = $s.locale === 'auto' ? $b : $s.locale;
  return isLocale(want) ? want : FALLBACK;
});

/** `$t('key', { n: 3 })` — `{n}` placeholders are replaced. Missing keys render the key. */
export const t = derived(locale, ($l) => (key: string, vars?: Record<string, string | number>): string => {
  let s = dicts[$l][key] ?? key;
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, String(v));
  return s;
});
