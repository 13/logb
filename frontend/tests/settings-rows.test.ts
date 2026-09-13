import { describe, expect, it } from 'vitest';
import { settingsRows, type SettingsRowsInput } from '../src/lib/settings-rows';

function input(over: Partial<SettingsRowsInput> = {}): SettingsRowsInput {
  return {
    isAdmin: false, username: 'ben', themeLabel: 'Dark', localeLabel: 'EN',
    tokenLabel: '2 keys', userLabel: '3 users', backendLabel: 'PostgreSQL', ...over,
  };
}

describe('settingsRows', () => {
  it('gives an ordinary user four rows, all in the "you" group', () => {
    const rows = settingsRows(input());
    expect(rows.map((r) => r.id)).toEqual(['appearance', 'account', 'api', 'data']);
    expect(rows.every((r) => r.group === 'you')).toBe(true);
  });

  it('adds the instance group for an administrator, after the personal rows', () => {
    const rows = settingsRows(input({ isAdmin: true }));
    expect(rows.map((r) => r.id)).toEqual(['appearance', 'account', 'api', 'data', 'people', 'database']);
    expect(rows.filter((r) => r.group === 'instance').map((r) => r.id)).toEqual(['people', 'database']);
  });

  /// `tokenLabel`/`userLabel` arrive already translated and already pluralised -- the caller
  /// picks the singular or plural key, and this module just carries whatever string it was
  /// given. The api/people values below use labels distinct from the fixture defaults so the
  /// assertion proves passthrough rather than merely matching `input()`'s own literals.
  it('carries the current value on each row, which is the point of the hub', () => {
    const rows = settingsRows(input({ isAdmin: true, tokenLabel: '1 Schlüssel', userLabel: '1 Benutzer' }));
    const value = (id: string) => rows.find((r) => r.id === id)?.value;
    expect(value('appearance')).toBe('Dark · EN');
    expect(value('account')).toBe('ben');
    expect(value('api')).toBe('1 Schlüssel');
    expect(value('people')).toBe('1 Benutzer');
    expect(value('database')).toBe('PostgreSQL');
  });

  /// A value that has not loaded yet must render as nothing at all. A placeholder or a zero
  /// would be a claim about the instance -- "no tokens", "SQLite" -- that nothing has checked.
  it('shows no value where the answer is not known yet, rather than guessing one', () => {
    const rows = settingsRows(input({ isAdmin: true, tokenLabel: null, userLabel: null, backendLabel: null }));
    const value = (id: string) => rows.find((r) => r.id === id)?.value;
    expect(value('api')).toBeNull();
    expect(value('people')).toBeNull();
    expect(value('database')).toBeNull();
  });

  /// Zero used to be special-cased inside this module ("0" printed nothing). That check has
  /// moved to the caller, which now folds a zero count into `null` before calling in -- a
  /// label is a decided fact by the time it reaches `settingsRows`, and this module must carry
  /// it through unchanged even if it happens to start with the digit "0".
  it('never re-derives a label on its own -- whatever string the caller decided on is shown verbatim', () => {
    expect(settingsRows(input({ tokenLabel: '0 keys' })).find((r) => r.id === 'api')?.value).toBe('0 keys');
  });

  it('never gives the data row a value, because it is a pair of actions and not a state', () => {
    expect(settingsRows(input()).find((r) => r.id === 'data')?.value).toBeNull();
  });

  it('routes every row under /settings', () => {
    for (const row of settingsRows(input({ isAdmin: true }))) {
      expect(row.path.startsWith('/settings/')).toBe(true);
    }
  });

  it('survives a session with no username, which is the state before /auth/me answers', () => {
    expect(settingsRows(input({ username: null })).find((r) => r.id === 'account')?.value).toBeNull();
  });
});
