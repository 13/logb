export const SUPPORTED = ['en', 'de'] as const;
export type Locale = (typeof SUPPORTED)[number];
export type LocalePref = Locale | 'auto';
export const FALLBACK: Locale = 'en';
export const LANG_NAMES: Record<Locale, string> = { en: 'English', de: 'Deutsch' };

export function isLocale(v: unknown): v is Locale {
  return typeof v === 'string' && (SUPPORTED as readonly string[]).includes(v);
}

export function pickLocale(tags: readonly string[]): Locale {
  for (const tag of tags) {
    const primary = String(tag ?? '').trim().toLowerCase().split('-')[0];
    if (isLocale(primary)) return primary;
  }
  return FALLBACK;
}

type NavigatorLike = { languages?: readonly string[]; language?: string };

export function navigatorTags(nav: NavigatorLike | undefined): readonly string[] {
  if (nav?.languages?.length) return nav.languages;
  return nav?.language ? [nav.language] : [];
}

export function detectLocale(nav: NavigatorLike | undefined = globalThis.navigator as NavigatorLike | undefined): Locale {
  return pickLocale(navigatorTags(nav));
}
