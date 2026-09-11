import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';

// The four that were there, plus the variation selector that often trails an emoji in source.
const EMOJI = /[\u{1F300}-\u{1FAFF}\u{2190}-\u{21FF}\u{2600}-\u{27BF}\u{FE0F}]/u;

const FILES = [
  '../src/lib/TopBar.svelte',
  '../src/routes/Dashboard.svelte',
  '../src/lib/Timeline.svelte',
];

describe('icons', () => {
  it('no emoji stand in for icons', () => {
    // They render differently on every platform and ignore the theme colour. This is the test
    // that stops one creeping back in the next time somebody wants a quick glyph.
    for (const f of FILES) {
      const src = readFileSync(new URL(f, import.meta.url), 'utf8');
      const hit = src.match(EMOJI);
      expect(hit, `${f} contains ${hit?.[0]}`).toBeNull();
    }
  });
});
