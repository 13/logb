import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { addTag, contrastRatio, foldTag, removeTag, suggestTags, tagColorIndex, TAG_PALETTE_SIZE } from '../src/lib/tags';

describe('tagColorIndex', () => {
  it('is stable and ignores case and accents', () => {
    expect(tagColorIndex('Winter')).toBe(tagColorIndex('winter'));
    expect(tagColorIndex('Fahrräder')).toBe(tagColorIndex('FAHRRADER'));
    expect(tagColorIndex('Lease')).toBe(tagColorIndex('Lease'));
  });
  it('stays inside the palette and spreads common tags over it', () => {
    const tags = ['winter', 'summer', 'lease', 'tax 2026', 'garage', 'warranty', 'work', 'family', 'loan', 'garden', 'kids', 'travel'];
    const used = new Set(tags.map(tagColorIndex));
    for (const i of used) expect(i).toBeGreaterThanOrEqual(0), expect(i).toBeLessThan(TAG_PALETTE_SIZE);
    expect(used.size).toBeGreaterThanOrEqual(5);
  });
});

describe('addTag / removeTag', () => {
  it('normalises and dedupes ignoring case and accents', () => {
    expect(addTag(['Winter'], '  winter ')).toEqual({ tags: ['Winter'] });
    expect(addTag(['Winter'], ' Garage   2 ')).toEqual({ tags: ['Winter', 'Garage 2'] });
  });
  it('refuses empty, too long and too many', () => {
    expect(addTag([], '   ')).toEqual({ error: 'empty' });
    expect(addTag([], 'x'.repeat(33))).toEqual({ error: 'too-long' });
    expect(addTag(Array.from({ length: 10 }, (_, i) => `t${i}`), 'eleventh')).toEqual({ error: 'too-many' });
  });
  it('removes by exact tag', () => {
    expect(removeTag(['a', 'b'], 'a')).toEqual(['b']);
  });
});

describe('suggestTags', () => {
  const all = [{ tag: 'Winter', count: 5 }, { tag: 'Wartung', count: 2 }, { tag: 'Lease', count: 9 }, { tag: 'Twin', count: 1 }];
  it('offers prefix matches before contains matches, and not what is already there', () => {
    expect(suggestTags(all, [], 'w')).toEqual(['Winter', 'Wartung', 'Twin']);
    expect(suggestTags(all, ['Winter'], 'win')).toEqual(['Twin']);
  });
  it('offers the most used tags when nothing is typed', () => {
    expect(suggestTags(all, [], '', 2)).toEqual(['Lease', 'Winter']);
  });
});

describe('palette contrast', () => {
  const css = readFileSync(fileURLToPath(new URL('../src/app.css', import.meta.url)), 'utf8');
  // Each theme block, as written in app.css.
  const blocks = {
    light: css.slice(css.indexOf(':root {'), css.indexOf('}', css.indexOf(':root {'))),
    dark: css.slice(css.indexOf(':root[data-theme="dark"] {'), css.indexOf('}', css.indexOf(':root[data-theme="dark"] {'))),
  };
  for (const [theme, block] of Object.entries(blocks)) {
    for (let n = 0; n < TAG_PALETTE_SIZE; n++) {
      it(`${theme} tag ${n} is readable (>= 4.5:1)`, () => {
        const bg = block.match(new RegExp(`--tag-${n}-bg:\\s*(#[0-9a-fA-F]{6})`))?.[1];
        const fg = block.match(new RegExp(`--tag-${n}-fg:\\s*(#[0-9a-fA-F]{6})`))?.[1];
        expect(bg, `--tag-${n}-bg in ${theme}`).toBeTruthy();
        expect(fg, `--tag-${n}-fg in ${theme}`).toBeTruthy();
        expect(contrastRatio(fg!, bg!)).toBeGreaterThanOrEqual(4.5);
      });
    }
  }
});

describe('foldTag / contrastRatio', () => {
  it('folds', () => expect(foldTag('Élan')).toBe('elan'));
  it('computes WCAG contrast', () => {
    expect(contrastRatio('#000000', '#ffffff')).toBeCloseTo(21, 0);
    expect(contrastRatio('#777777', '#ffffff')).toBeCloseTo(4.48, 1);
  });
});
