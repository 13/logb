import { persisted } from './persisted';
import type { LocalePref } from '../i18n/detect';

export interface LocalSettings {
  locale: LocalePref;
  theme: 'auto' | 'light' | 'dark';
}

export const settings = persisted<LocalSettings>('memto.settings', { locale: 'auto', theme: 'auto' });
