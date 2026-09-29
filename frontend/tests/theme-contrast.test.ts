import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { contrastRatio } from '../src/lib/tags.ts';

/**
 * WCAG 2.1 AA for the colour pairs the stylesheet itself sets: text on a status chip and the warn
 * text need 4.5:1, a form control's border 3:1 against what it sits on (1.4.11, non-text).
 * Read from app.css so a later palette tweak cannot quietly undo it.
 */
const css = readFileSync(fileURLToPath(new URL('../src/app.css', import.meta.url)), 'utf8');
const block = (sel: string) => css.slice(css.indexOf(sel), css.indexOf('}', css.indexOf(sel)));
const light = block(':root {');
const dark = block(':root[data-theme="dark"] {');
const token = (name: string, theme: string): string => {
  const own = theme.match(new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{6})`))?.[1];
  // A token the dark block does not redefine is inherited from :root.
  const value = own ?? light.match(new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{6})`))?.[1];
  expect(value, `--${name}`).toBeTruthy();
  return value!;
};
/** The one declaration of `selector` in app.css. */
const rule = (selector: string) => {
  const at = css.indexOf(`\n${selector} {`);
  expect(at, selector).toBeGreaterThan(-1);
  return css.slice(at, css.indexOf('}', at));
};
const varIn = (decl: string, prop: string) => decl.match(new RegExp(`(?:^|[\\s;{])${prop}:[^;]*var\\(--([a-z0-9-]+)\\)`))?.[1];

describe('theme contrast', () => {
  for (const [name, theme] of [['light', light], ['dark', dark]] as const) {
    for (const chip of ['.chip.due', '.chip.pending', '.chip.dead']) {
      it(`${name}: ${chip} text is readable (>= 4.5:1)`, () => {
        const r = rule(chip);
        const bg = varIn(r, 'background'); const fg = varIn(r, 'color');
        expect(bg && fg, `${chip} uses tokens for both colours`).toBeTruthy();
        expect(contrastRatio(token(fg!, theme), token(bg!, theme))).toBeGreaterThanOrEqual(4.5);
      });
    }

    it(`${name}: the field warning is readable on the page and on a card (>= 4.5:1)`, () => {
      const fg = varIn(rule('.field .warn'), 'color')!;
      for (const bg of ['bg', 'surface']) expect(contrastRatio(token(fg, theme), token(bg, theme)), bg).toBeGreaterThanOrEqual(4.5);
    });

    it(`${name}: a form control's border can be seen (>= 3:1)`, () => {
      const border = varIn(rule(':is(input, select, textarea):where(:not([data-slot]))'), 'border')!;
      for (const bg of ['bg', 'surface']) expect(contrastRatio(token(border, theme), token(bg, theme)), bg).toBeGreaterThanOrEqual(3);
    });
  }
});
