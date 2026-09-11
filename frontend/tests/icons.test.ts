import { describe, expect, it } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, relative } from 'node:path';

// The four that were there originally, plus the variation selector that often trails an emoji
// in source, plus the arrow/dingbat blocks a couple of icon-like emoji (📷, 📄, ...) don't cover
// on their own but that earlier sweeps still used loosely as "emoji".
const EMOJI = /[\u{1F300}-\u{1FAFF}\u{2190}-\u{21FF}\u{2600}-\u{27BF}\u{FE0F}]/u;

const SRC_ROOT = fileURLToPath(new URL('../src', import.meta.url));

// A hardcoded file list is how this test missed two emoji last time (ActivityForm.svelte's own
// doc-chip, FilePicker.svelte's camera) even though a near-identical one had already been fixed
// in Timeline.svelte. Walk every .svelte file under src instead, so a new one can't slip past
// the same way.
function svelteFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...svelteFiles(full));
    else if (entry.isFile() && entry.name.endsWith('.svelte')) out.push(full);
  }
  return out;
}

// Files that legitimately contain a character the EMOJI pattern matches, but that isn't an
// emoji standing in for an icon -- excluded explicitly rather than narrowing the pattern.
//
// `char` names the EXACT known glyph the exception is for. The main test below only skips that
// one occurrence, not the whole file, and the self-check confirms `char` specifically is still
// there (not just that *something* EMOJI-shaped is) -- so a file that stops needing its
// exception fails loudly, and a *different* glyph later added to an excluded file isn't
// silently covered by someone else's exemption.
const EXCLUSIONS: Record<string, { char: string; reason: string }> = {
  'App.svelte': {
    char: '→',
    reason:
      "the arrow in `// Route table: pattern → [component, param names]` is inside a source " +
      'comment, not rendered UI.',
  },
};

describe('icons', () => {
  const files = svelteFiles(SRC_ROOT).map((f) => ({ abs: f, rel: relative(SRC_ROOT, f) }));

  it('found the svelte files under src (sanity check the walk itself works)', () => {
    expect(files.length).toBeGreaterThan(10);
  });

  it('no emoji stand in for icons', () => {
    // They render differently on every platform and ignore the theme colour. This is the test
    // that stops one creeping back in the next time somebody wants a quick glyph.
    for (const { abs, rel } of files) {
      let src = readFileSync(abs, 'utf8');
      const exclusion = EXCLUSIONS[rel];
      if (exclusion) {
        // Strip only the exact excused occurrence(s) before scanning, so a *different* emoji
        // added anywhere in this file -- including right next to the excused one -- still fails.
        src = src.split(exclusion.char).join('');
      }
      const hit = src.match(EMOJI);
      expect(hit, `${rel} contains ${hit?.[0]}`).toBeNull();
    }
  });

  it('every exclusion still exists and still needs its exception', () => {
    // Guards against the exclusion list going stale: a file that got fixed (or removed) should
    // come out of EXCLUSIONS, not sit there unused -- and this checks for the SPECIFIC excused
    // character, not merely that the file still matches the general EMOJI pattern (which a
    // brand-new, unrelated glyph-as-icon would also satisfy, masking a real regression).
    const relPaths = new Set(files.map((f) => f.rel));
    for (const [rel, { char }] of Object.entries(EXCLUSIONS)) {
      expect(relPaths.has(rel), `${rel} is excluded but no longer exists`).toBe(true);
      const src = readFileSync(join(SRC_ROOT, rel), 'utf8');
      expect(src.includes(char), `${rel} is excluded for ${char} but no longer contains it -- remove the exception`).toBe(true);
    }
  });
});
