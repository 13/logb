import { describe, expect, it } from 'vitest';
import { fieldError } from '../src/lib/form-error';
import en from '../src/i18n/en';

/** The same interpolation the app's `t` does, over the real English table. */
function t(key: string, vars: Record<string, string | number> = {}): string {
  const text = (en as Record<string, string>)[key] ?? key;
  return text.replace(/\{(\w+)\}/g, (_, name) => String(vars[name] ?? `{${name}}`));
}

describe('fieldError', () => {
  it('says what to do about the field, rather than printing its label alone', () => {
    const message = fieldError('activity.title', t);
    expect(message).not.toBe('Title');
    expect(message).toBe('Check “Title”: it is missing or not valid.');
  });

  it('names whichever field the validator returned', () => {
    expect(fieldError('activity.cost', t)).toContain('“Cost”');
  });
});
