import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { addTag, contrastRatio, foldTag, removeTag, splitTyped, suggestTags, tagColorIndex, TAG_PALETTE_SIZE } from '../src/lib/tags.ts';

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

describe('splitTyped', () => {
  it('commits a completed segment and keeps the rest as text', () => {
    expect(splitTyped(['A'], 'b,')).toEqual({ tags: ['A', 'b'], text: '', error: null });
  });
  it('commits every completed segment, trimming stray spaces', () => {
    expect(splitTyped([], 'a, b ,c')).toEqual({ tags: ['a', 'b'], text: 'c', error: null });
  });
  it('dedupes a segment against an existing tag without raising an error', () => {
    expect(splitTyped(['Winter'], 'winter,x')).toEqual({ tags: ['Winter'], text: 'x', error: null });
  });
  it('leaves a too-long segment in the text and reports the error, still committing the rest', () => {
    const value = `${'x'.repeat(33)},ok,`;
    expect(splitTyped([], value)).toEqual({ tags: ['ok'], text: 'x'.repeat(33), error: 'too-long' });
  });
  it('leaves a segment that would overflow the tag limit in the text and reports the error', () => {
    const ten = Array.from({ length: 10 }, (_, i) => `t${i}`);
    expect(splitTyped(ten, 'eleven,')).toEqual({ tags: ten, error: 'too-many', text: 'eleven' });
  });
  it('passes text through unchanged when there is no comma', () => {
    expect(splitTyped(['A'], 'partial')).toEqual({ tags: ['A'], text: 'partial', error: null });
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
  // The same table is asserted against Rust's `fold` in src/domain/tags.rs.
  it.each([
    ['ß', 'ß'], ['ẞ', 'ß'], ['İ', 'i'], ['ΟΔΟΣ', 'οδος'],
    ['Fahrräder', 'fahrrader'], ['Élan Vital', 'elan vital'], ['INFO', 'info'],
  ])('folds %s like the server does', (input, folded) => expect(foldTag(input)).toBe(folded));
  it('computes WCAG contrast', () => {
    expect(contrastRatio('#000000', '#ffffff')).toBeCloseTo(21, 0);
    expect(contrastRatio('#777777', '#ffffff')).toBeCloseTo(4.48, 1);
  });
});
