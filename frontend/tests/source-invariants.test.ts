import { describe, expect, it } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';

const srcDir = new URL('../src/', import.meta.url);

function sources(dir: URL, base = ''): Array<{ path: string; text: string }> {
  const out: Array<{ path: string; text: string }> = [];
  for (const name of readdirSync(dir)) {
    const url = new URL(name, dir);
    if (statSync(url).isDirectory()) out.push(...sources(new URL(`${name}/`, dir), `${base}${name}/`));
    else if (/\.(ts|svelte)$/.test(name)) out.push({ path: `${base}${name}`, text: readFileSync(url, 'utf8') });
  }
  return out;
}

describe('temp id minting', () => {
  const files = sources(srcDir);

  it('has one hash definition: hashToNegativeId', () => {
    const hashes = files.filter((f) => /\*\s*31\s*\+/.test(f.text)).map((f) => f.path);
    expect(hashes).toEqual(['lib/activity-form.ts']);
  });

  it('calls crypto.randomUUID unguarded nowhere but newOpId, which falls back on plain http', () => {
    const bare = files.filter((f) => /crypto\.randomUUID\(\)/.test(f.text) && f.path !== 'lib/outbox.ts').map((f) => f.path);
    expect(bare).toEqual([]);
  });
});
