import { describe, expect, it } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, relative } from 'node:path';

// Wider than "emoji" strictly means, deliberately: the pictographic block covers 📷 and 📄,
// and the arrow and dingbat blocks are here for ← and ⚙, which are not emoji by any Unicode
// property but were standing in for icons all the same. The Halfwidth and Fullwidth Forms block
// is here for ＋, which stood in for the dashboard's quick-log button for weeks because this
// pattern didn't cover it. The working definition is "a glyph used as an icon", and the pattern
// matches that rather than Emoji_Presentation.
const EMOJI = /[\u{1F300}-\u{1FAFF}\u{2190}-\u{21FF}\u{2600}-\u{27BF}\u{FE0F}\u{FF00}-\u{FFEF}]/u;

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
// `char` names the EXACT known glyph the exception is for, and the exemption is scoped to that
// character rather than to the file: every occurrence of `char` is stripped before scanning, so
// a *different* glyph later added to an excluded file still fails. (A second, genuinely rendered
// `→` in App.svelte would be excused -- accepted, because the alternative is pinning a line
// number that ordinary edits would churn.) The self-check then confirms `char` specifically is
// still present, so a file that stops needing its exception fails loudly instead of sitting
// there unused.
const EXCLUSIONS: Record<string, { char: string; reason: string }> = {
  'App.svelte': {
    char: '→',
    reason:
      "the arrow in `// Route table: pattern → [component, param names]` is inside a source " +
      'comment, not rendered UI.',
  },
};

// Read as text rather than imported: this file is type-checked under the Node tsconfig, which
// cannot resolve a `.svelte` import, and a type is not a value a test could iterate anyway.
function stringList(src: string, pattern: RegExp): string[] {
  const m = src.match(pattern);
  if (!m) return [];
  return [...m[1].matchAll(/['"]([^'"]+)['"]/g)].map((x) => x[1]);
}

describe('own type icons', () => {
  const iconNames = stringList(readFileSync(join(SRC_ROOT, 'lib/icon-names.ts'), 'utf8'), /export type IconName =([^;]+);/);
  const client = stringList(readFileSync(join(SRC_ROOT, 'lib/type-registry.ts'), 'utf8'), /CUSTOM_TYPE_ICONS: IconName\[\] = \[([^\]]+)\]/);
  const server = stringList(
    readFileSync(fileURLToPath(new URL('../../src/domain/custom_type.rs', import.meta.url)), 'utf8'),
    /CUSTOM_TYPE_ICONS: &\[&str\] = &\[([^\]]+)\]/,
  );

  it('parsed all three lists (sanity check the patterns still match)', () => {
    expect(iconNames.length).toBeGreaterThan(20);
    expect(client.length).toBeGreaterThan(5);
    expect(server.length).toBeGreaterThan(5);
  });

  // An icon the server accepts but Icon.svelte cannot draw would render as an empty square.
  it('every icon a type may have is one Icon.svelte draws', () => {
    for (const icon of client) expect(iconNames, icon).toContain(icon);
  });

  // The picker offering an icon the server refuses is a Save that always fails.
  it('the picker offers exactly the icons the server accepts', () => {
    expect(client).toEqual(server);
  });
});

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
