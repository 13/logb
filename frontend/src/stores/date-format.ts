import { derived, type Readable } from 'svelte/store';
import { settings } from './settings';
import { locale, navigatorLangs } from '../i18n';
import { resolveDateFormat, type DateFormat } from '../lib/format';

/** `navigatorLangs` (not just `locale`) is a dependency so a `languagechange` that only changes
 *  the REGION -- `en-GB` to `en-US`, say, with the primary language staying `en` -- still
 *  re-resolves `auto`; see the comment on `navigatorLangs` in `../i18n/index.ts`. */
export const dateFormat: Readable<DateFormat> = derived([settings, locale, navigatorLangs], ([$s, $l, $langs]) =>
  resolveDateFormat($s.dateFormat ?? 'auto', $l, $langs));
