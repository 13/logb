import { derived, get, writable } from 'svelte/store';
import { settings } from '../stores/settings';
import { detectLocale, isLocale, navigatorTags, FALLBACK, type Locale } from './detect';
import en from './en';

/**
 * The fallback language is in the main bundle; every other one is its own chunk, loaded when it
 * becomes the active locale (see `ensureLocale`). Nobody reads the app in two languages at once,
 * so shipping both to everyone made every start parse a dictionary it would never show. The
 * service worker precaches every chunk, so a language switch works offline too.
 */
type Dict = Record<string, string>;
const loaders: Partial<Record<Locale, () => Promise<{ default: Dict }>>> = {
  de: () => import('./de'),
};
/** The dictionaries in hand. A store, so `t` re-derives the moment one arrives. */
const dicts = writable<Partial<Record<Locale, Dict>>>({ en });
const pending = new Map<Locale, Promise<void>>();

/** Loads `l`'s dictionary if it is not in hand yet. Resolves once it is (or once loading it has
 *  failed: `t` then keeps showing the fallback language rather than bare keys). */
export function ensureLocale(l: Locale): Promise<void> {
  const load = loaders[l];
  if (get(dicts)[l] || !load) return Promise.resolve();
  let p = pending.get(l);
  if (!p) {
    p = load()
      .then((m: { default: Dict }) => { dicts.update((d) => ({ ...d, [l]: m.default })); })
      .catch((e: unknown) => { pending.delete(l); console.warn(`locale ${l} could not be loaded`, e); });
    pending.set(l, p);
  }
  return p;
}

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

// Whatever becomes the active locale is fetched straight away; `main.ts` waits for the first one
// before mounting, so a start never shows the fallback language for a moment.
locale.subscribe((l) => { void ensureLocale(l); });

/** `$t('key', { n: 3 })` — `{n}` placeholders are replaced. Missing keys render the key. A
 *  language still loading (a switch in Settings) shows the fallback language meanwhile. */
export const t = derived([locale, dicts], ([$l, $d]) => (key: string, vars?: Record<string, string | number>): string => {
  let s = $d[$l]?.[key] ?? $d[FALLBACK]?.[key] ?? key;
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, String(v));
  return s;
});
