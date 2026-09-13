import { describe, expect, it } from 'vitest';
import { settingsRows, type SettingsRowsInput } from '../src/lib/settings-rows';

function input(over: Partial<SettingsRowsInput> = {}): SettingsRowsInput {
  return {
    isAdmin: false, username: 'ben', themeLabel: 'Dark', localeLabel: 'EN',
    tokenCount: 2, userCount: 3, backendLabel: 'PostgreSQL', ...over,
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

  it('carries the current value on each row, which is the point of the hub', () => {
    const rows = settingsRows(input({ isAdmin: true }));
    const value = (id: string) => rows.find((r) => r.id === id)?.value;
    expect(value('appearance')).toBe('Dark · EN');
    expect(value('account')).toBe('ben');
    expect(value('api')).toBe('2 keys');
    expect(value('people')).toBe('3 users');
    expect(value('database')).toBe('PostgreSQL');
  });

  /// A value that has not loaded yet must render as nothing at all. A placeholder or a zero
  /// would be a claim about the instance -- "no tokens", "SQLite" -- that nothing has checked.
  it('shows no value where the answer is not known yet, rather than guessing one', () => {
    const rows = settingsRows(input({ isAdmin: true, tokenCount: null, userCount: null, backendLabel: null }));
    const value = (id: string) => rows.find((r) => r.id === id)?.value;
    expect(value('api')).toBeNull();
    expect(value('people')).toBeNull();
    expect(value('database')).toBeNull();
  });

  it('says nothing rather than "0 keys" when there are none', () => {
    expect(settingsRows(input({ tokenCount: 0 })).find((r) => r.id === 'api')?.value).toBeNull();
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
