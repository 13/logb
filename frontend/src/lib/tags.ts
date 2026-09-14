// `.js`: tests/tags.test.ts is type-checked under tsconfig.node.json (nodenext), which needs an
// extension here; the browser config's bundler resolution maps it to types.ts all the same.
import type { TagCount } from './types.js';

export const TAG_PALETTE_SIZE = 8;
export const MAX_TAGS = 10;
export const MAX_TAG_CHARS = 32;

/** Not `toLocaleLowerCase`: that follows the device's locale (Turkish "INFO" becomes "ınfo"),
 *  while the server's `domain::tags::fold` does not. */
export function foldTag(tag: string): string {
  return tag.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
}

/** A tag's palette slot, from its folded name: the same tag gets the same colour on every device
 *  with nothing stored. FNV-1a is small, fast and spreads short strings well. */
export function tagColorIndex(tag: string): number {
  let hash = 0x811c9dc5;
  for (const ch of foldTag(tag)) {
    hash ^= ch.codePointAt(0)!;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash % TAG_PALETTE_SIZE;
}

export function normalizeTag(raw: string): string {
  return raw.trim().replace(/\s+/g, ' ');
}

export type AddResult = { tags: string[] } | { error: 'too-long' | 'too-many' | 'empty' };

export function addTag(tags: string[], raw: string): AddResult {
  const tag = normalizeTag(raw);
  if (!tag) return { error: 'empty' };
  if ([...tag].length > MAX_TAG_CHARS) return { error: 'too-long' };
  if (tags.some((t) => foldTag(t) === foldTag(tag))) return { tags };
  if (tags.length >= MAX_TAGS) return { error: 'too-many' };
  return { tags: [...tags, tag] };
}

export function removeTag(tags: string[], tag: string): string[] {
  return tags.filter((t) => t !== tag);
}

export type SplitResult = { tags: string[]; text: string; error: 'too-long' | 'too-many' | null };

/**
 * Applies every comma-terminated segment of a freshly typed value as a tag, via `addTag`, and
 * keeps whatever follows the last comma as the text still being typed. A segment that `addTag`
 * refuses (too long, or the tag limit already reached) stays in the text too, ahead of what
 * follows -- not silently dropped -- so it can be edited rather than retyped; an empty segment
 * (two commas in a row, or trailing spaces) is skipped without error. When two segments fail,
 * `error` simply ends up holding the LATER one's -- each failure overwrites it in turn, nothing
 * more deliberate than that. Every failed segment stays in the text regardless (see above), so
 * an earlier one is not hidden or lost; only its message is not the one shown until it is fixed.
 */
export function splitTyped(tags: string[], value: string): SplitResult {
  if (!value.includes(',')) return { tags, text: value, error: null };
  const parts = value.split(',');
  const rest = parts.pop() ?? '';
  let current = tags;
  let error: 'too-long' | 'too-many' | null = null;
  const failed: string[] = [];
  for (const part of parts) {
    if (!part.trim()) continue;
    const r = addTag(current, part);
    if ('error' in r) {
      if (r.error === 'empty') continue;
      failed.push(part);
      error = r.error;
    } else {
      current = r.tags;
    }
  }
  const text = (rest !== '' ? [...failed, rest] : failed).join(',');
  return { tags: current, text, error };
}

export function suggestTags(all: TagCount[], current: string[], text: string, limit = 8): string[] {
  const have = new Set(current.map(foldTag));
  const q = foldTag(normalizeTag(text));
  const candidates = all.filter((t) => !have.has(foldTag(t.tag)));
  if (!q) return [...candidates].sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag)).slice(0, limit).map((t) => t.tag);
  const prefix = candidates.filter((t) => foldTag(t.tag).startsWith(q));
  const contains = candidates.filter((t) => !foldTag(t.tag).startsWith(q) && foldTag(t.tag).includes(q));
  return [...prefix, ...contains].slice(0, limit).map((t) => t.tag);
}

/** WCAG 2.x contrast ratio between two `#rrggbb` colours. */
export function contrastRatio(fg: string, bg: string): number {
  const lum = (hex: string) => {
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
      .map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const [hi, lo] = [lum(fg), lum(bg)].sort((a, b) => b - a);
  return (hi + 0.05) / (lo + 0.05);
}
