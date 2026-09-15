import { persisted } from './persisted';
import type { LocalePref } from '../i18n/detect';
import type { DateFormatPref } from '../lib/format';

export interface LocalSettings {
  locale: LocalePref;
  theme: 'auto' | 'light' | 'dark';
  dateFormat: DateFormatPref;
}

export const settings = persisted<LocalSettings>('logb.settings', { locale: 'auto', theme: 'auto', dateFormat: 'auto' });
