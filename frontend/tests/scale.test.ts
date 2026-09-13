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
 * Values a component may hold despite not being on the scale, each with the reason. Listed by
 * the exact string so an exemption that stops being needed fails loudly, the way the icon
 * test's exclusions do.
 */
const ALLOWED: Record<string, string> = {
  '1px': "the pending badge's vertical inset, drawn over a 64px thumbnail: optical, not spatial",
  '2px': 'the focus ring and badge insets are optical, not spatial',
  '0': 'zero is zero',
  auto: 'a centring keyword, not a spacing value: `margin: 0 auto` is alignment',
};

/** Every spacing token any component writes, scale values included. */
function spacingTokens(): string[] {
  return styleBlocks(SRC).flatMap(({ css }) =>
    [...css.matchAll(/(?:gap|margin|padding)(?:-[a-z]+)?:\s*([^;]+);/g)].flatMap((m) =>
      m[1].trim().split(/\s+/),
    ),
  );
}

describe('the scale', () => {
  it('no component invents a font size', () => {
    for (const { rel, css } of styleBlocks(SRC)) {
      const hits = [...css.matchAll(/font-size:\s*([^;]+);/g)].map((m) => m[1].trim());
      const raw = hits.filter((v) => !v.startsWith('var(--text-'));
      expect(raw, `${rel} sets a font size outside the scale: ${raw.join(', ')}`).toEqual([]);
    }
  });

  it('no component invents a spacing value', () => {
    for (const { rel, css } of styleBlocks(SRC)) {
      const hits = [...css.matchAll(/(?:gap|margin|padding)(?:-[a-z]+)?:\s*([^;]+);/g)]
        .flatMap((m) => m[1].trim().split(/\s+/))
        .filter((v) => !v.startsWith('var(--space-') && !(v in ALLOWED));
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
