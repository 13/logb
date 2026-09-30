import { describe, expect, it } from 'vitest';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, relative } from 'node:path';

const SRC = fileURLToPath(new URL('../src', import.meta.url));

function svelteFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const full = join(dir, e.name);
    return e.isDirectory() ? svelteFiles(full) : e.name.endsWith('.svelte') ? [full] : [];
  });
}

/**
 * Styling is Tailwind utilities and the tokens and primitives in app.tw.css, nothing else. A
 * scoped `<style>` block is unlayered and beats every utility, so one left behind silently
 * overrides the class list beside it. The checks this file used to hold kept those blocks on
 * app.css's spacing, type and radius scale; the blocks and the scale are both gone.
 */
describe('styling', () => {
  it('no component carries a <style> block', () => {
    const withStyle = svelteFiles(SRC).filter((f) => /<style[\s>]/.test(readFileSync(f, 'utf8'))).map((f) => relative(SRC, f));
    expect(withStyle).toEqual([]);
  });

  it('app.css is gone, and app.tw.css keeps no layer or mention of it', () => {
    expect(existsSync(join(SRC, 'app.css'))).toBe(false);
    expect(readFileSync(join(SRC, 'app.tw.css'), 'utf8')).not.toMatch(/legacy|app\.css/);
  });

  it('no markup uses a class name app.css used to define', () => {
    const old = /class="(?:[^"]*\s)?(field|row|toggle|hint|warn|warning|error|card|list|muted|empty|empty-icon|chip|banner|button-like|primary|danger|ghost|tnum|small|fab|topbar|auth|settings-grid|thumb-grid)(?:\s[^"]*)?"|class:(primary|ghost|danger)=/;
    const hits = svelteFiles(SRC).filter((f) => old.test(readFileSync(f, 'utf8'))).map((f) => relative(SRC, f));
    expect(hits).toEqual([]);
  });
});
