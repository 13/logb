/**
 * The arithmetic behind loading an object's timeline, kept out of `ObjectDetail.svelte` so it
 * can be unit-tested. Every bug this file's logic has had -- a chunk loop that never
 * terminated, an offset that skipped rows, a guaranteed-empty second request on every ordinary
 * load, a reload gate that missed another tab's work -- was reachable only through Playwright
 * while it lived in the component, which is slow enough that some of it went unnoticed for
 * several rounds. None of it needs a DOM.
 */

/** One page of results as the API returns it. */
export interface PageResult<T> {
  items: T[];
  total: number;
}

/**
 * - `reset` starts over from page one: what a filter or object change wants.
 * - `append` fetches the page after what is on screen.
 * - `refresh` re-reads everything already on screen without shrinking it back to one page.
 */
export type LoadMode = 'reset' | 'append' | 'refresh';

/** Rows per page, and the server's own cap on `limit`. */
export const PAGE_SIZE = 100;

/**
 * Must equal `MAX_LIMIT` in `src/api/activities.rs`. The server CLAMPS a larger `limit` instead
 * of refusing it, so a value that drifts above the real cap loses rows with a successful
 * response and nothing to notice. `tests/activities.rs` reads this declaration and fails if the
 * two ever disagree, pins that the server clamps rather than refuses, and
 * `timeline-load.test.ts` pins that no single request asks for more than this.
 */
export const MAX_LIMIT = 500;

/**
 * How wide a window to ask for, and where it starts.
 *
 * `loaded` counts only rows the SERVER sent back -- a synthetic pending entry has no page
 * position at all, so counting it would shift every later request back by one real row and skip
 * it. A refresh covers everything already pulled in, because asking for a single page would
 * silently discard every "load more" the user did.
 */
export function windowFor(mode: LoadMode, loaded: number): { want: number; base: number; append: boolean } {
  const append = mode === 'append';
  return {
    append,
    want: mode === 'refresh' ? Math.max(PAGE_SIZE, loaded) : PAGE_SIZE,
    base: append ? loaded : 0,
  };
}

/**
 * Fetches `want` rows from `base`, in chunks the server will accept, dropping any row a chunk
 * repeats.
 *
 * Two things here are load-bearing, and each was a bug first:
 *
 * The offset advances by the rows RECEIVED, never by the rows kept. Chunks are separate
 * requests over one `ORDER BY`, so a row inserted at the top between them shifts everything
 * down and the next chunk repeats rows the previous one already had. If the offset advanced by
 * what survived deduplication, a full-length chunk of entirely duplicate rows would leave it
 * exactly where it was and re-issue the identical request for ever.
 *
 * The loop stops on a SHORT page, not only an empty one. Stopping only at zero costs every
 * ordinary object -- one with fewer activities than a page -- a second, guaranteed-empty round
 * trip and the `COUNT(*)` that comes with it.
 */
export async function fetchWindow<T extends { id: number }>(
  want: number,
  base: number,
  fetchPage: (limit: number, offset: number) => Promise<PageResult<T>>,
): Promise<PageResult<T>> {
  let items: T[] = [];
  let total = 0;
  let received = 0;
  while (items.length < want) {
    const limit = Math.min(MAX_LIMIT, want - items.length);
    const page = await fetchPage(limit, base + received);
    received += page.items.length;
    const seen = new Set(items.map((a) => a.id));
    items = [...items, ...page.items.filter((a) => !seen.has(a.id))];
    total = page.total;
    // Two stop conditions, and the loop needs both.
    //
    // A SHORT page means the server had nothing more to give. Stopping only at zero costs every
    // ordinary object -- one with fewer activities than a page -- a second, guaranteed-empty
    // round trip and the `COUNT(*)` that comes with it.
    if (page.items.length < limit) break;
    // Having walked past every row the server says exists, there is nothing left to ask for.
    // Advancing the offset by rows received guarantees progress, but progress alone does not
    // guarantee an end: a server that answers every offset with a full page -- one that ignores
    // `offset`, or a proxy serving one cached response -- would otherwise be asked for ever,
    // since deduplication means `items` stops growing. This bounds the loop by what the server
    // itself reports rather than by trusting it to run out.
    if (received >= total) break;
  }
  return { items, total };
}

/**
 * Whether a completed outbox pass is a reason to reload the timeline.
 *
 * Two conditions, because neither covers the other:
 *
 * `changed` is what THIS tab's pass did, and it is the only thing that sees a queued upload
 * land -- an upload changes an existing entry's thumbnails and the object's stats, and never
 * appears as a row of its own, so no comparison of rendered rows can detect it.
 *
 * It says nothing about a pass another TAB ran, though: that tab sends and removes the op, and
 * this tab's next pass then sees an empty queue before and after and reports no change, leaving
 * a dimmed pending row on screen for a write that landed minutes ago. Comparing what is
 * rendered against what the queue now holds catches that -- and, unlike the sampled key this
 * replaced, it cannot go stale, because both sides are read at the moment of the decision.
 */
export function shouldReload(changed: boolean, renderedPendingIds: number[], queuedPendingIds: number[]): boolean {
  if (changed) return true;
  const key = (ids: number[]) => [...ids].sort((a, b) => a - b).join(',');
  return key(renderedPendingIds) !== key(queuedPendingIds);
}

/**
 * The list to render: an append extends what is on screen, anything else replaces it, with the
 * queued-but-unsent entries in front. Those come first because they are the newest thing the
 * user did and will not appear in any page the server sends back until the outbox replays them.
 */
export function mergeWindow<T>(append: boolean, existing: T[], pending: T[], items: T[]): T[] {
  return append ? [...existing, ...items] : [...pending, ...items];
}
