import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';

const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');

describe('viewport', () => {
  // Chrome for Android then shrinks the layout viewport when the keyboard opens, so the sticky
  // Save bar (FormActions) sits above the keyboard instead of behind it. iOS ignores the key.
  it('lets the on-screen keyboard resize the layout', () => {
    expect(html).toMatch(/<meta name="viewport" content="[^"]*interactive-widget=resizes-content[^"]*"/);
  });
});
