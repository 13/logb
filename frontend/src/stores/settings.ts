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

/** What this device remembers about one account's appearance. */
export interface AccountAppearance {
  /** What the server last said this account's appearance is, or null until it has said. */
  account: LocalSettings | null;
  /** What this device last showed while that account was signed in. */
  device: LocalSettings | null;
  /** When set, this device keeps its own appearance and stops following the account. */
  override: boolean;
}

const defaults: LocalSettings = { locale: 'auto', theme: 'auto', dateFormat: 'auto', firstDayOfWeek: 'locale' };
const blank: AccountAppearance = { account: null, device: null, override: false };

/** The appearance in force right now. The UI subscribes to this and nothing else. */
export const settings = persisted<LocalSettings>('logb.settings', defaults);

/**
 * Every account this device has seen, keyed by id.
 *
 * One record rather than three parallel maps: the three pieces are only ever read together, and
 * keeping them apart meant a write to one could land without the others -- a device override
 * stored for an account whose cached appearance had already been dropped, say.
 */
export const appearance = persisted<Record<string, AccountAppearance>>('logb.appearance', {});

let owner: number | null = null;

function entry(id: number): AccountAppearance {
  return get(appearance)[String(id)] ?? blank;
}

function patch(id: number, change: Partial<AccountAppearance>): void {
  appearance.update(all => ({ ...all, [String(id)]: { ...(all[String(id)] ?? blank), ...change } }));
}

// Whatever the signed-in account is looking at is what this device shows it next time.
settings.subscribe(value => {
  if (owner !== null) patch(owner, { device: value });
});

/**
 * Says whose appearance `settings` now holds: an account id at sign-in, null at sign-out.
 *
 * Switching owner swaps the store's contents rather than keeping them, so one account's theme
 * never shows up under another's name on a shared device.
 */
export function appearanceOwner(id: number | null): void {
  if (owner === id) return;
  // A device that has never had an account signed in keeps what it was set to; one that has
  // starts from the defaults instead of inheriting the last account's look.
  const unseen = Object.keys(get(appearance)).length === 0 ? get(settings) : defaults;
  owner = null;
  if (id === null) {
    settings.set(defaults);
  } else {
    const known = entry(id);
    settings.set(known.device ?? known.account ?? unseen);
  }
  owner = id;
}

/** The value to hand back to `applyAccountAppearance` as proof of what was on screen. */
export function appearanceSnapshot(): LocalSettings {
  return get(settings);
}

function same(a: LocalSettings, b: LocalSettings): boolean {
  return a.locale === b.locale && a.theme === b.theme && a.dateFormat === b.dateFormat
    && a.firstDayOfWeek === b.firstDayOfWeek;
}

/**
 * Takes what the server says this account's appearance is.
 *
 * `issuedWith` is the appearance that was on screen when the request went out. If it no longer
 * matches, somebody changed a setting while the request was in flight and the answer is stale:
 * the value is still remembered for next time, but it does not overwrite the newer choice.
 */
export function applyAccountAppearance(id: number, value: LocalSettings | null, issuedWith?: LocalSettings): void {
  if (owner !== id || !value) return;
  patch(id, { account: value });
  if (entry(id).override) return;
  if (issuedWith && !same(issuedWith, get(settings))) return;
  settings.set(value);
}

export function setDeviceOverride(id: number, enabled: boolean): void {
  patch(id, { override: enabled });
}
