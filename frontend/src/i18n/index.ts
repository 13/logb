import { derived, get, writable } from 'svelte/store';
import { settings } from '../stores/settings';
import { detectLocale, isLocale, navigatorTags, FALLBACK, SUPPORTED, type Locale } from './detect';

/**
 * Every language is its own chunk, the fallback included, loaded when it becomes the active
 * locale (see `ensureLocale`): nobody reads the app in two languages at once, so the entry chunk
 * carries no dictionary at all. `main.ts` mounts only once the active one is in hand. English is
 * not loaded alongside another language as a per-key fallback: `tests/i18n.test.ts` keeps both
 * dictionaries at the same keys and placeholders instead. The service worker precaches every
 * chunk, so a start or a language switch works offline too.
 */
type Dict = Record<string, string>;
const loaders: Record<Locale, () => Promise<{ default: Dict }>> = {
  en: () => import('./en'),
  de: () => import('./de'),
};
/** The dictionaries in hand. A store, so `t` re-derives the moment one arrives. */
const dicts = writable<Partial<Record<Locale, Dict>>>({});
const pending = new Map<Locale, Promise<boolean>>();

/** Loads `l`'s dictionary unless it is in hand; true once it is. A failure is not remembered
 *  here, but Chromium remembers a failed dynamic import for the page's lifetime, so in practice a
 *  language that failed arrives with the next start (a retry on `online` was tried and never
 *  succeeded there). */
function load(l: Locale): Promise<boolean> {
  if (get(dicts)[l]) return Promise.resolve(true);
  let p = pending.get(l);
  if (!p) {
    p = loaders[l]()
      .then((m: { default: Dict }) => { dicts.update((d) => ({ ...d, [l]: m.default })); return true; })
      .catch((e: unknown) => { console.warn(`locale ${l} could not be loaded`, e); return false; })
      .finally(() => pending.delete(l));
    pending.set(l, p);
  }
  return p;
}

/** Loads `l`'s dictionary if it is not in hand yet, and resolves once it is. Should that fail (a
 *  chunk missing offline, a deploy that replaced it), some language must still be there to show
 *  rather than bare keys: the fallback language is loaded instead, then any other, unless one is
 *  already in hand. `t` shows whichever it has. */
export async function ensureLocale(l: Locale): Promise<void> {
  if (await load(l)) return;
  for (const other of [FALLBACK, ...SUPPORTED]) {
    if (other !== l && (get(dicts)[other] || await load(other))) return;
  }
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
// before mounting, so a start never shows another language (or bare keys) for a moment.
locale.subscribe((l) => { void ensureLocale(l); });

/** `$t('key', { n: 3 })` — `{n}` placeholders are replaced. Missing keys render the key. A
 *  language not in hand yet (a switch in Settings, or one that failed to load) is stood in for by
 *  the language already on screen, so the page never shows bare keys meanwhile. */
export const t = derived([locale, dicts], ([$l, $d]) => (key: string, vars?: Record<string, string | number>): string => {
  const dict = $d[$l] ?? $d[FALLBACK] ?? Object.values($d)[0];
  let s = dict?.[key] ?? key;
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, String(v));
  return s;
});
