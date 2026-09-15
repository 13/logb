import { derived, type Readable } from 'svelte/store';
import { settings } from './settings';
import { browserLang, locale } from '../i18n';
import { navigatorTags } from '../i18n/detect';
import { resolveDateFormat, type DateFormat } from '../lib/format';

/** `browserLang` is only a dependency so a `languagechange` re-resolves the English region. */
export const dateFormat: Readable<DateFormat> = derived([settings, locale, browserLang], ([$s, $l]) =>
  resolveDateFormat($s.dateFormat ?? 'auto', $l, navigatorTags(globalThis.navigator)));
