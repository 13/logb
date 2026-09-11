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
const EXCLUSIONS: Record<string, string> = {
  'App.svelte':
    "the arrow in `// Route table: pattern → [component, param names]` is inside a source " +
    'comment, not rendered UI.',
  'lib/Reminders.svelte':
    'the ↻ after a due date is a plain-text repeat-indicator glyph (no emoji presentation, ' +
    'renders in currentColor like any other character) -- not a decorative emoji needing an SVG icon.',
  'routes/ObjectDetail.svelte':
    'the ✎ in the edit button is the same kind of plain-text symbol as the ↻ above, already ' +
    'consistent across platforms and themes.',
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
      if (rel in EXCLUSIONS) continue;
      const src = readFileSync(abs, 'utf8');
      const hit = src.match(EMOJI);
      expect(hit, `${rel} contains ${hit?.[0]}`).toBeNull();
    }
  });

  it('every exclusion still exists and still needs its exception', () => {
    // Guards against the exclusion list going stale: a file that got fixed (or removed) should
    // come out of EXCLUSIONS, not sit there unused.
    const relPaths = new Set(files.map((f) => f.rel));
    for (const rel of Object.keys(EXCLUSIONS)) {
      expect(relPaths.has(rel), `${rel} is excluded but no longer exists`).toBe(true);
      const src = readFileSync(join(SRC_ROOT, rel), 'utf8');
      expect(src.match(EMOJI), `${rel} is excluded but no longer matches EMOJI -- remove the exception`).not.toBeNull();
    }
  });
});
