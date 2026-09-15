import { newOpId } from './outbox';
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
 * Also stamped with a fresh one-time token, returned here so the caller can carry it through
 * Types' own URL (`draft=<token>`) and hand it back to `takeObjectDraft`. A draft restores only
 * when that exact token comes back with it -- so merely LANDING on the object form again (a
 * plain visit to `/objects/new`, the back button, a second tab, the next person on a shared
 * device) never resurrects somebody else's abandoned input; only the one round trip that just
 * minted the token can claim it. `newOpId` is reused here purely as a random-id generator --
 * this draft is never sent to the server or replayed as an outbox op.
 *
 * All storage access is wrapped: a draft is a convenience, and a blocked or full
 * `sessionStorage` must not stop the navigation it is part of.
 */
export function saveObjectDraft(returnPath: string, input: ObjectInput): string {
  const token = newOpId();
  try {
    sessionStorage.setItem(KEY, JSON.stringify({ path: returnPath, token, input }));
  } catch { /* the shortcut still navigates; the form just comes back empty */ }
  return token;
}

/**
 * Reads back a draft saved for exactly `returnPath` under exactly `token`, and removes whatever
 * was stored either way -- so it is used at most once, and a mount that carries the wrong token
 * (or none at all) both gets nothing back AND discards the stored draft, rather than leaving it
 * sitting there for a later, unrelated visit to pick up.
 */
export function takeObjectDraft(returnPath: string, token: string | null): ObjectInput | null {
  try {
    const raw = sessionStorage.getItem(KEY);
    if (raw === null) return null;
    sessionStorage.removeItem(KEY);
    const parsed = JSON.parse(raw) as { path?: unknown; token?: unknown; input?: unknown };
    if (token === null || parsed.path !== returnPath || parsed.token !== token) return null;
    return parsed.input as ObjectInput;
  } catch {
    return null;
  }
}

/**
 * Drops any kept draft outright, unconditionally. Called when a session ends (`endSession` in
 * `../stores/session.ts`): `sessionStorage` is one store per TAB, shared by whoever signs in
 * next on it (a shared device, or the same person signing back in), and the token check alone
 * does not protect against that handoff -- the browser's own history can still carry the exact
 * `draft=<token>` URL from before sign-out (a back button, a reopened tab), which would restore
 * the previous session's input straight into the next one's form. Clearing the key here removes
 * it regardless of what token a later visit presents.
 */
export function forgetObjectDraft(): void {
  try { sessionStorage.removeItem(KEY); } catch { /* nothing to clear */ }
}
