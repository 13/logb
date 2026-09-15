import type { ObjectInput } from './types';

const KEY = 'logb.object-draft';

/**
 * A same-app object-form path, and nothing else -- the only shapes `return` may legally name.
 *
 * `return` arrives on Types' own URL after a round trip through the "+ New type…" shortcut (see
 * `ObjectForm.svelte`), so it is attacker-controllable query text, not a value this app itself
 * always produced: an absolute or protocol-relative address, or any path outside the object
 * form, is refused rather than followed.
 */
export function safeReturnPath(raw: string | null): string | null {
  if (raw === null) return null;
  return /^\/objects\/(new|\d+\/edit)$/.test(raw) ? raw : null;
}

/**
 * Keeps the object form's current input while the "+ New type…" shortcut visits Types and
 * comes back. Keyed by the return path too, so a draft saved for one form (a second tab, an
 * old navigation) is never picked up by another.
 *
 * All storage access is wrapped: a draft is a convenience, and a blocked or full
 * `sessionStorage` must not stop the navigation it is part of.
 */
export function saveObjectDraft(returnPath: string, input: ObjectInput): void {
  try {
    sessionStorage.setItem(KEY, JSON.stringify({ path: returnPath, input }));
  } catch { /* the shortcut still navigates; the form just comes back empty */ }
}

/**
 * Reads back a draft saved for exactly `returnPath` and removes it either way, so it is used at
 * most once -- a second read (a reload, a different path) finds nothing left to restore.
 */
export function takeObjectDraft(returnPath: string): ObjectInput | null {
  try {
    const raw = sessionStorage.getItem(KEY);
    if (raw === null) return null;
    sessionStorage.removeItem(KEY);
    const parsed = JSON.parse(raw) as { path?: unknown; input?: unknown };
    return parsed.path === returnPath ? (parsed.input as ObjectInput) : null;
  } catch {
    return null;
  }
}
