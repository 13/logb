import { describe, expect, it, vi } from 'vitest';
import {
  fetchWindow, MAX_LIMIT, mergeWindow, PAGE_SIZE, shouldReload, windowFor,
  type PageResult,
} from '../src/lib/timeline-load';

/** A server holding `total` rows, ids 0..total-1, newest first, clamping `limit` the way the
 *  real one does. `shift` simulates rows inserted at the top between requests. */
function server(total: number, shift = 0) {
  const calls: Array<{ limit: number; offset: number }> = [];
  const fetchPage = async (limit: number, offset: number): Promise<PageResult<{ id: number }>> => {
    calls.push({ limit, offset });
    const clamped = Math.min(limit, MAX_LIMIT);
    const from = Math.max(0, offset - shift);
    return {
      items: Array.from({ length: Math.max(0, Math.min(clamped, total - from)) }, (_, i) => ({ id: from + i })),
      total,
    };
  };
  return { calls, fetchPage };
}

describe('windowFor', () => {
  it('asks for one page from the start on a reset', () => {
    expect(windowFor('reset', 0)).toEqual({ want: PAGE_SIZE, base: 0, append: false });
    expect(windowFor('reset', 350)).toEqual({ want: PAGE_SIZE, base: 0, append: false });
  });

  it('asks for the page after what is on screen on an append', () => {
    expect(windowFor('append', 250)).toEqual({ want: PAGE_SIZE, base: 250, append: true });
  });

  /** The regression: a refresh that asked for one page threw away every "load more" the user
   *  had done, and a flush runs on every `visibilitychange`. */
  it('covers everything already loaded on a refresh, never less than a page', () => {
    expect(windowFor('refresh', 350)).toEqual({ want: 350, base: 0, append: false });
    expect(windowFor('refresh', 12)).toEqual({ want: PAGE_SIZE, base: 0, append: false });
  });
});

describe('fetchWindow', () => {
  it('returns one page in one request for an ordinary object', async () => {
    const { calls, fetchPage } = server(3);
    const page = await fetchWindow(PAGE_SIZE, 0, fetchPage);

    expect(page.items.map((a) => a.id)).toEqual([0, 1, 2]);
    expect(page.total).toBe(3);
    // Stopping only on an EMPTY page would make this two requests, on every load of every
    // object with fewer activities than a page -- and a COUNT(*) with each.
    expect(calls).toHaveLength(1);
  });

  it('stops as soon as the window is full', async () => {
    const { calls, fetchPage } = server(1000);
    const page = await fetchWindow(PAGE_SIZE, 0, fetchPage);

    expect(page.items).toHaveLength(PAGE_SIZE);
    expect(calls).toEqual([{ limit: PAGE_SIZE, offset: 0 }]);
  });

  it('splits a window wider than the server cap into chunks it will accept', async () => {
    const { calls, fetchPage } = server(1000);
    const page = await fetchWindow(600, 0, fetchPage);

    expect(page.items).toHaveLength(600);
    expect(calls).toEqual([{ limit: MAX_LIMIT, offset: 0 }, { limit: 100, offset: 500 }]);
    // No id twice: `Timeline`'s {#each} is keyed by id and throws on a duplicate.
    expect(new Set(page.items.map((a) => a.id)).size).toBe(600);
  });

  it('starts from the offset it is given', async () => {
    const { calls, fetchPage } = server(1000);
    const page = await fetchWindow(PAGE_SIZE, 250, fetchPage);

    expect(calls).toEqual([{ limit: PAGE_SIZE, offset: 250 }]);
    expect(page.items[0].id).toBe(250);
  });

  /**
   * Chunks are separate requests over one ORDER BY, so rows inserted at the top between them
   * shift everything down and the next chunk repeats rows the previous one already returned.
   */
  it('drops rows a later chunk repeats after an insert shifts the ordering', async () => {
    const { fetchPage } = server(1000, 2);
    const page = await fetchWindow(600, 0, fetchPage);

    expect(new Set(page.items.map((a) => a.id)).size).toBe(page.items.length);
  });

  /**
   * The one that hung the app: with the offset advancing by rows KEPT rather than rows
   * RECEIVED, a full-length chunk of entirely duplicate rows left it where it was and the same
   * request went out for ever -- unbounded, never resolving, with the spinner stuck on.
   *
   * This server is worse than that bug needed: it ignores `offset` entirely and answers every
   * request with the same full page, so deduplication means `items` never grows past the first
   * chunk. An advancing offset alone does not end that -- only counting against the `total` the
   * server reports does.
   */
  it('terminates against a server that answers every offset with the same full page', async () => {
    let call = 0;
    const fetchPage = async (limit: number): Promise<PageResult<{ id: number }>> => {
      if (++call > 20) throw new Error('fetchWindow did not terminate');
      return { items: Array.from({ length: limit }, (_, i) => ({ id: i })), total: 1000 };
    };

    const page = await fetchWindow(600, 0, fetchPage);

    expect(call).toBeLessThan(20);
    expect(page.items.map((a) => a.id)).toEqual(Array.from({ length: MAX_LIMIT }, (_, i) => i));
  });

  it('stops when the server runs out mid-window', async () => {
    const { calls, fetchPage } = server(620);
    const page = await fetchWindow(1000, 0, fetchPage);

    expect(page.items).toHaveLength(620);
    expect(calls).toHaveLength(2);
  });

  it('lets a failure through rather than returning a half-filled window', async () => {
    const fetchPage = vi.fn(async () => { throw new Error('offline'); });
    await expect(fetchWindow(PAGE_SIZE, 0, fetchPage)).rejects.toThrow('offline');
  });
});

describe('shouldReload', () => {
  it('reloads whenever this tab\'s own pass changed the queue', () => {
    // The only signal that sees a queued UPLOAD land: it changes an existing entry's thumbnails
    // and the object's stats, and never appears as a row of its own.
    expect(shouldReload(true, [], [])).toBe(true);
    expect(shouldReload(true, [-7], [-7])).toBe(true);
  });

  it('does not reload when nothing happened', () => {
    // Passes run on every `visibilitychange`, overwhelmingly over an empty queue: reloading
    // here means re-fetching the timeline every time the tab regains focus.
    expect(shouldReload(false, [], [])).toBe(false);
    expect(shouldReload(false, [-7, -9], [-9, -7])).toBe(false);
  });

  /**
   * Another tab's pass sends and removes the op; this tab's next pass then sees an empty queue
   * before and after and reports no change, while the dimmed pending row is still on screen.
   */
  it('reloads when what is rendered no longer matches the queue', () => {
    expect(shouldReload(false, [-7], [])).toBe(true);
    expect(shouldReload(false, [], [-7])).toBe(true);
    expect(shouldReload(false, [-7], [-9])).toBe(true);
  });
});

describe('mergeWindow', () => {
  it('puts queued entries in front of the server page', () => {
    expect(mergeWindow(false, ['old'], ['queued'], ['a', 'b'])).toEqual(['queued', 'a', 'b']);
  });

  it('extends what is on screen on an append, without re-adding pending entries', () => {
    expect(mergeWindow(true, ['queued', 'a'], [], ['b'])).toEqual(['queued', 'a', 'b']);
  });
});
