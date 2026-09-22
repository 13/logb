import { persisted } from './persisted';
import { get } from 'svelte/store';
import type { LocalePref } from '../i18n/detect';
import type { DateFormatPref } from '../lib/format';

export interface LocalSettings {
  locale: LocalePref;
  theme: 'auto' | 'light' | 'dark';
  dateFormat: DateFormatPref;
  firstDayOfWeek: 'locale' | 'monday' | 'sunday';
}

const defaults: LocalSettings = { locale: 'auto', theme: 'auto', dateFormat: 'auto', firstDayOfWeek: 'locale' };
export const settings = persisted<LocalSettings>('logb.settings', defaults);

let owner: number | null = null;
export const deviceOverride = persisted<Record<string, boolean>>('logb.appearance-overrides', {});
const accountCache = persisted<Record<string, LocalSettings>>('logb.appearance-accounts', {});
const deviceCache = persisted<Record<string, LocalSettings>>('logb.appearance-devices', {});
let revision = 0;
export function appearanceRevision(): number { return revision; }
settings.subscribe(value => {
  revision++;
  if (owner !== null) deviceCache.update(all => ({ ...all, [String(owner)]: value }));
});
export function appearanceOwner(id: number | null): void {
  if (owner === id) return;
  const initial = Object.keys(get(deviceCache)).length === 0 ? get(settings) : defaults;
  owner = null;
  const cached = id === null ? defaults : get(deviceCache)[String(id)] ?? get(accountCache)[String(id)] ?? initial;
  settings.set(cached);
  owner = id;
}
export function applyAccountAppearance(id: number, value: LocalSettings | null, expectedRevision?: number): void {
  if (owner !== id || !value) return;
  accountCache.update(all => ({ ...all, [String(id)]: value }));
  if (!get(deviceOverride)[String(id)] && (expectedRevision === undefined || expectedRevision === revision)) settings.set(value);
}
