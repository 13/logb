import { describe, expect, it } from 'vitest';
import { existsSync, readFileSync } from 'node:fs';
import { pwaIcons } from '../scripts/pwa-icons.ts';

const publicDir = new URL('../public/', import.meta.url);

describe('pwa icons', () => {
  it('names only files that exist', () => {
    // The failure this catches: somebody edits icon.svg, forgets to re-run the render script,
    // and ships a bee on the browser tab with the old artwork on the phone home screen.
    for (const icon of pwaIcons) {
      expect(existsSync(new URL(icon.src, publicDir)), `${icon.src} is missing`).toBe(true);
    }
  });

  it('declares exactly one maskable icon', () => {
    const maskable = pwaIcons.filter((i) => i.purpose?.includes('maskable'));
    expect(maskable).toHaveLength(1);
    // A maskable icon must be drawn with a safe zone. Declaring the tight one as maskable --
    // which the manifest did before this change -- gets its edges clipped by the launcher.
    expect(maskable[0].src).toBe('pwa-512-maskable.png');
  });

  it('keeps the markers the render script slices on', () => {
    const svg = readFileSync(new URL('icon.svg', publicDir), 'utf8');
    expect(svg).toContain('<!--bee-->');
    expect(svg).toContain('<!--/bee-->');
  });
});
