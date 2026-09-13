import { describe, expect, it } from 'vitest';
import { DESTINATIONS, activeDestination } from '../src/lib/nav';

describe('activeDestination', () => {
  it('marks Objects on the dashboard', () => {
    expect(activeDestination('/')).toBe('objects');
  });

  it('marks Objects on a drill-down below it, so a nav does not go blank three screens deep', () => {
    expect(activeDestination('/objects/42')).toBe('objects');
    expect(activeDestination('/objects/new')).toBe('objects');
    expect(activeDestination('/objects/42/activities/new')).toBe('objects');
  });

  it('marks Search', () => {
    expect(activeDestination('/search')).toBe('search');
  });

  it('marks Settings, including its sub-pages', () => {
    expect(activeDestination('/settings')).toBe('settings');
    expect(activeDestination('/settings/appearance')).toBe('settings');
  });

  it('marks nothing where the shell does not render', () => {
    expect(activeDestination('/login')).toBeNull();
    expect(activeDestination('/setup')).toBeNull();
  });

  /// A prefix match on the bare string would light up Objects for any path that merely starts
  /// with those letters. The boundary has to be a path separator, not a character count.
  it('does not match a path that merely starts with a destination name', () => {
    expect(activeDestination('/objectsfoo')).toBeNull();
    expect(activeDestination('/searching')).toBeNull();
    expect(activeDestination('/settingsx')).toBeNull();
  });

  it('lists exactly the three top-level destinations, in order', () => {
    expect(DESTINATIONS.map((d) => d.id)).toEqual(['objects', 'search', 'settings']);
    expect(DESTINATIONS.map((d) => d.path)).toEqual(['/', '/search', '/settings']);
  });
});
