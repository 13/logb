import { describe, expect, it } from 'vitest';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, relative } from 'node:path';

const SRC = fileURLToPath(new URL('../src', import.meta.url));

/** Every `<style>` block under src, as (file, css) pairs. */
function styleBlocks(dir: string): Array<{ rel: string; css: string }> {
  const out: Array<{ rel: string; css: string }> = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...styleBlocks(full));
    else if (entry.name.endsWith('.svelte')) {
      const src = readFileSync(full, 'utf8');
      const m = src.match(/<style>([\s\S]*?)<\/style>/);
      if (m) out.push({ rel: relative(SRC, full), css: m[1] });
    }
  }
  return out;
}

/**
 * The declarations that space a box, and the custom properties that alias them. A value
 * laundered through `--space-something: 7px` is still a spacing value the component invented,
 * so the alias is read at its declaration rather than where it is used -- otherwise one line
 * of CSS buys an exemption from the whole scale.
 *
 * The value ends on `[;}]`, not on `;`: CSS lets the last declaration in a block drop its
 * semicolon, and `gap: 7px }` is exactly as much a spacing value as `gap: 7px;` is. Requiring
 * the semicolon made that declaration invisible -- and, where a later one did carry a
 * semicolon, made the match run past the closing brace and report the next rule's selector.
 */
const SPACING = /(?:(?:gap|margin|padding)(?:-[a-z]+)*|--space-[\w-]*):\s*([^;}]+)[;}]/g;

/**
 * The values a declaration writes, with scale references and `calc()` scaffolding taken out.
 * What is left is what the component chose for itself: `calc(44px + var(--space-2) * 2)` is
 * one invented value (`44px`) and one scale reference, not five tokens.
 */
function values(raw: string, scale: RegExp): string[] {
  return raw
    .replace(scale, ' ')
    // A unitless factor inside a `calc()` is arithmetic on a value, not a value of its own:
    // the `2` in `var(--space-2) * 2` doubles a scale step rather than inventing a number.
    .replace(/[*/]\s*[\d.]+|[\d.]+\s*[*/]/g, ' ')
    .replace(/\bcalc\b|[()*/+]/g, ' ')
    .split(/\s+/)
    .filter(Boolean);
}

const spacingValues = (raw: string) => values(raw, /var\(--space-[\w-]*\)/g);

/**
 * Values a component may hold despite not being on the scale, each with the reason. Listed by
 * the exact string so an exemption that stops being needed fails loudly, the way the icon
 * test's exclusions do.
 */
const ALLOWED: Record<string, string> = {
  '1px': "the pending badge's vertical inset, drawn over a 64px thumbnail: optical, not spatial",
  '2px': 'the focus ring and badge insets are optical, not spatial',
  '44px': 'the tap-target floor the quick-log gutter is built from: an accessibility minimum, not a spacing step',
  '0': 'zero is zero',
  auto: 'a centring keyword, not a spacing value: `margin: 0 auto` is alignment',
};

/** Every spacing token any component writes, scale values included. */
function spacingTokens(): string[] {
  return styleBlocks(SRC).flatMap(({ css }) =>
    [...css.matchAll(SPACING)].flatMap((m) => spacingValues(m[1])),
  );
}

describe('the scale', () => {
  it('no component invents a font size', () => {
    for (const { rel, css } of styleBlocks(SRC)) {
      // `[;}]` for the same reason SPACING uses it: `font-size: 13px }` is a font size.
      const hits = [...css.matchAll(/font-size:\s*([^;}]+)[;}]/g)].map((m) => m[1].trim());
      const raw = hits.filter((v) => !v.startsWith('var(--text-'));
      expect(raw, `${rel} sets a font size outside the scale: ${raw.join(', ')}`).toEqual([]);
    }
  });

  it('no component invents a spacing value', () => {
    for (const { rel, css } of styleBlocks(SRC)) {
      const hits = [...css.matchAll(SPACING)]
        .flatMap((m) => spacingValues(m[1]))
        .filter((v) => !(v in ALLOWED));
      expect(hits, `${rel} sets spacing outside the scale: ${hits.join(', ')}`).toEqual([]);
    }
  });

  it('every exemption is still needed', () => {
    // The same guard the icon test carries: an exemption whose value no longer appears anywhere
    // should come out of ALLOWED rather than sit there excusing nothing. `50%` and `100%` came
    // out this way -- neither is ever written as a gap, a margin or a padding.
    const written = new Set(spacingTokens());
    for (const [value, reason] of Object.entries(ALLOWED)) {
      expect(
        written.has(value),
        `no component writes ${value} as spacing any more (${reason}) -- remove the exemption`,
      ).toBe(true);
    }
  });
});
