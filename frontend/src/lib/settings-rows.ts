import type { IconName } from './Icon.svelte';

export type SettingsGroup = 'you' | 'instance';

/** One row of the settings hub. `label` is an i18n key; `value` is text already rendered for
 *  display, or `null` to show nothing at all. */
export type SettingsRowModel = {
  id: string;
  path: string;
  icon: IconName;
  label: string;
  value: string | null;
  group: SettingsGroup;
};

export type SettingsRowsInput = {
  isAdmin: boolean;
  username: string | null;
  /** Already-translated, e.g. "Dark" -- this module does no lookups. */
  themeLabel: string;
  /** An uppercase language code, e.g. "EN". */
  localeLabel: string;
  /** Already-translated, singular/plural already chosen by the caller, e.g. "1 key" or
   *  "3 keys" -- this module does no lookups. `null` until `/auth/tokens` answers, and also
   *  when there are none: the caller turns a zero count into `null` too. */
  tokenLabel: string | null;
  /** Already-translated, singular/plural already chosen by the caller, e.g. "1 user" or
   *  "3 users". `null` until `/users` answers, and for a non-admin who never asks. */
  userLabel: string | null;
  /** `null` until `/database` answers, e.g. "PostgreSQL". */
  backendLabel: string | null;
  /** Already-translated, e.g. "On for 2 of your devices"; `null` until `/me/notifications`
   *  answers, and when nothing is set up. */
  notificationsLabel: string | null;
};

/** What the hub shows, and what each row says about itself.
 *
 *  Every value here is either a fact the caller already has or `null`. A row whose answer has
 *  not arrived shows nothing rather than a placeholder: "SQLite" or "0 keys" on a screen is a
 *  claim about the instance, and a hub that guesses is worse than one that waits. */
export function settingsRows(input: SettingsRowsInput): SettingsRowModel[] {
  const rows: SettingsRowModel[] = [
    {
      id: 'appearance', path: '/settings/appearance', icon: 'palette', label: 'settings.appearance',
      value: `${input.themeLabel} · ${input.localeLabel}`, group: 'you',
    },
    {
      id: 'account', path: '/settings/account', icon: 'person', label: 'settings.account',
      value: input.username, group: 'you',
    },
    {
      id: 'notifications', path: '/settings/notifications', icon: 'bell', label: 'settings.notifications',
      value: input.notificationsLabel, group: 'you',
    },
    {
      // Per user, like everything in this group: nobody sees anyone else's types.
      id: 'types', path: '/settings/types', icon: 'object', label: 'settings.types',
      value: null, group: 'you',
    },
    {
      id: 'api', path: '/settings/api', icon: 'key', label: 'tokens.title',
      // Zero is not a number worth printing here: "no keys" is the default state of every
      // account, and a row that says so is noise on a screen meant to be scanned. The caller
      // is the one that turns a zero count into `null` -- this module just passes it through.
      value: input.tokenLabel, group: 'you',
    },
    {
      id: 'data', path: '/settings/data', icon: 'box', label: 'settings.data',
      value: null, group: 'you',
    },
  ];
  if (!input.isAdmin) return rows;
  rows.push(
    {
      id: 'people', path: '/settings/people', icon: 'people', label: 'settings.users',
      value: input.userLabel, group: 'instance',
    },
    {
      id: 'database', path: '/settings/database', icon: 'database', label: 'db.title',
      value: input.backendLabel, group: 'instance',
    },
  );
  return rows;
}
