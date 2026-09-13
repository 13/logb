import { describe, expect, it } from 'vitest';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { pwaIcons } from '../scripts/pwa-icons.ts';

const publicDir = new URL('../public/', import.meta.url);
const scriptsDir = new URL('../scripts/', import.meta.url);

describe('pwa icons', () => {
  it('names only files that exist', () => {
    // This only catches a declared file going missing entirely, not stale artwork -- see
    // "PNGs match the current icon.svg" below for the check that catches staleness.
    for (const icon of pwaIcons) {
      expect(existsSync(new URL(icon.src, publicDir)), `${icon.src} is missing`).toBe(true);
    }
  });

  it('PNGs match the current icon.svg', () => {
    // The failure this catches: somebody edits icon.svg, forgets to re-run the render script,
    // and ships the new mark on the browser tab with the old artwork on the phone home screen. The
    // PNGs still exist and the manifest still declares them, so nothing else notices.
    const svg = readFileSync(new URL('icon.svg', publicDir));
    const actual = createHash('sha256').update(svg).digest('hex');
    const stamped = readFileSync(new URL('icon.sha256', scriptsDir), 'utf8').trim();
    expect(
      actual,
      'public/icon.svg has changed since the PNGs were last rendered -- run ' +
        'frontend/scripts/render-icons.sh and commit the regenerated PNGs and icon.sha256'
    ).toBe(stamped);
  });

  it('declares exactly one maskable icon', () => {
    const maskable = pwaIcons.filter((i) => i.purpose?.includes('maskable'));
    expect(maskable).toHaveLength(1);
    // A maskable icon must be drawn with a safe zone. Declaring the tight one as maskable --
    // which the manifest did before this change -- gets its edges clipped by the launcher.
    expect(maskable[0].src).toBe('pwa-512-maskable.png');
  });

  it('keeps the markers the render script and Logo.svelte slice on', () => {
    const svg = readFileSync(new URL('icon.svg', publicDir), 'utf8');
    const start = svg.indexOf('<!--mark-->');
    const end = svg.indexOf('<!--/mark-->');
    expect(start).toBeGreaterThan(-1);
    expect(end).toBeGreaterThan(start);
    // Logo.svelte recolours the mark by swapping this exact ink for currentColor. Drawn in any
    // other spelling of white, the in-app logo would stay white and vanish on the light theme.
    const mark = svg.slice(start, end);
    expect(mark).toContain('#ffffff');
    expect(mark.replaceAll('#ffffff', '')).not.toMatch(/#[0-9a-f]{3,8}\b|\bwhite\b/i);
  });

  it('gives iOS its 180px icon', () => {
    const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
    expect(html).toContain('href="/apple-touch-icon.png"');
    expect(existsSync(new URL('apple-touch-icon.png', publicDir))).toBe(true);
  });
});
