import { readonly, writable, type Readable } from 'svelte/store';
import { type QueuedOp } from './outbox';
import { ApiError, isRejection, isUnauthenticated } from './api-error';
import { path as routerPath } from './router';

export { ApiError, isRejection, isUnauthenticated } from './api-error';

/**
 * Which request paths (exactly as passed to `api()`/`apiPage()`/etc, query string included) are
 * currently answering from the service worker's cache rather than the network -- see
 * `servedFromCache` below. A per-path SET, not one flag: most screens have several requests in
 * flight at once (the dashboard alone fires five), and a single "last response wins" flag used to
 * flap back to false the instant any ONE of them came back fresh -- an uncached `/settings`
 * answering in milliseconds, say -- while another was still visibly showing data from `logb-api`.
 *
 * A key is added by a stale response and removed only by a FRESH response to that SAME path,
 * never by an unrelated one -- or by the screen replacing that query in place (`supersedeStale`).
 * The whole set is dropped on a route change (`clearServingSaved`, wired to the router's `path`
 * below -- each screen re-fetches what it needs, so staleness recorded for the previous screen's
 * requests stops being meaningful) and when a session ends (same function, called from
 * `endSession` in ../stores/session.ts).
 */
const staleKeys = new Set<string>();
const servingSavedState = writable(false);
export const servingSaved: Readable<boolean> = readonly(servingSavedState);

/**
 * Bumped every time `clearServingSaved` runs (a route change, or a session ending). A request
 * captures the CURRENT generation next to `sentAt` when it is sent; `handle` below only touches
 * `staleKeys` for a response whose captured generation still matches -- otherwise the request
 * was sent for a screen the user has since navigated away from (or a session that has since
 * ended), and a stale answer arriving late for it must not re-add a key `clearServingSaved` has
 * already dropped, resurrecting a note for a screen nobody is looking at any more.
 */
let routeGeneration = 0;

/** Drops all tracked staleness, hides the note, and starts a new generation (see
 *  `routeGeneration`) so a response already in flight for the screen just left cannot re-add
 *  its key once it finally arrives. The generation bump is unconditional -- a route change with
 *  nothing currently stale still needs to invalidate anything already in flight -- but the set
 *  clear and store update are skipped when there is nothing to do. */
export function clearServingSaved(): void {
  routeGeneration++;
  // Every request sent so far now belongs to an old generation, so no prefix cutoff below can
  // matter any more; dropping them keeps the map from growing with every object visited.
  supersededUpTo.clear();
  if (staleKeys.size === 0) return;
  staleKeys.clear();
  servingSavedState.set(false);
}

/** Numbers every request as it is sent, so `supersedeStale` can tell requests made before a
 *  query was replaced from those made after. */
let requestSeq = 0;

/** Path prefix -> the last request number sent before that prefix's query was replaced. */
const supersededUpTo = new Map<string, number>();

/**
 * For a screen that replaces what it shows WITHOUT a route change -- a category or tag filter on
 * the activity timeline re-fetches under a new query string. Staleness recorded for the old
 * query would otherwise keep the note up over a list that now came fresh from the network, since
 * only a fresh answer to that exact old query (never asked again) or a navigation cleared it.
 * Drops every stale key under `prefix` and ignores answers to requests under it that were sent
 * before this call, so a slow answer to the old query cannot put the key back. Requests sent
 * afterwards are judged normally.
 */
export function supersedeStale(prefix: string): void {
  supersededUpTo.set(prefix, requestSeq);
  let dropped = false;
  for (const key of staleKeys) {
    if (key.startsWith(prefix)) {
      staleKeys.delete(key);
      dropped = true;
    }
  }
  if (dropped) servingSavedState.set(staleKeys.size > 0);
}

/**
 * For a screen that could not reach the network at all and fell back to data it saved itself
 * (outside the service worker, so no response ever reached `handle`). Without this, superseding
 * the old query before a load that then fails would hide the note over saved or empty data once
 * the connection dropped mid-session, since `offline` mode is only set when the app starts
 * offline. Use a key under the prefix the screen supersedes, so its next load clears it.
 */
export function markServingSaved(key: string): void {
  staleKeys.add(key);
  servingSavedState.set(true);
}

function isSuperseded(path: string, seq: number): boolean {
  for (const [prefix, upTo] of supersededUpTo) {
    if (seq <= upTo && path.startsWith(prefix)) return true;
  }
  return false;
}
// Each screen re-fetches what it shows on mount, so navigating away makes any staleness recorded
// for the PREVIOUS screen's requests meaningless -- without this, a note earned by one slow load
// on the objects list would keep showing on a completely unrelated screen that never touched that
// path. `path` only changes on a real `popstate` (see ./router.ts), which needs a DOM `window` --
// this subscription is inert, harmlessly, under Vitest's node test environment; `clearServingSaved`
// is exported so a test can simulate a route change directly instead.
routerPath.subscribe(() => clearServingSaved());

/**
 * How far the SERVER's clock reads from this device's, in ms (positive: the server is behind).
 * A self-hosted instance with no RTC (a Raspberry Pi that boots believing it's 1970, or simply the
 * wrong timezone) can be off by far more than the 60s threshold below, in either direction --
 * which would otherwise show the note permanently (server ahead of us) or hide a genuine cache hit
 * forever (server behind us, so a stale cached response still looks "recent enough").
 *
 * Calibrated from responses that can NEVER be a cache hit: `/auth/...` and `/settings` are
 * `NetworkOnly` (see vite.config.ts), so their `Date` header always reflects a live request made
 * moments ago -- any gap between it and roughly when the server saw the request is clock skew,
 * not cache age. Both are requested on every session check, so this recalibrates itself
 * continuously rather than trusting one reading for the life of the tab. See `handle` below for
 * how it is measured -- the midpoint of the round trip, and only for a round trip short enough
 * that the midpoint is a trustworthy stand-in for "when the server wrote that header".
 */
let clockSkewMs = 0;

/** How long a calibrating response's round trip may take and still be trusted. Past this, the
 *  midpoint below stops being a reasonable proxy for "roughly when the server wrote its `Date`
 *  header" -- a response that took 70s to come back could have spent nearly all of that queued
 *  or retried, nowhere near the midpoint of the interval, and calibrating from it anyway once
 *  corrupted `clockSkewMs` badly enough to make every later FRESH response look cached. */
const MAX_CALIBRATION_ROUND_TRIP_MS = 5_000;

/** Test seam only: resets the calibrated skew between tests that exercise it through
 *  `api()`/`handle()`, so one test's calibration cannot leak into the next. Production never
 *  calls this -- the module starts at 0 and only ever recalibrates from a real response. */
export function resetClockSkewForTesting(): void {
  clockSkewMs = 0;
}

/**
 * Pure so it can be unit-tested without a fetch: true only when `dateHeader` is far enough before
 * `sentAt` (the moment the request went out), once `skewMs` -- the server clock's known offset
 * from this device's, see `clockSkewMs` above -- is subtracted out, that ordinary latency cannot
 * explain it. Given the 4s `NetworkFirst` timeout, only a cache hit reads as over a minute stale
 * after that correction. A missing or unparsable header counts as fresh: there is nothing there to
 * prove otherwise, and treating "we don't know" as "cached" would show the note on every response
 * an unrelated proxy happened to strip the header from.
 */
export function servedFromCache(dateHeader: string | null, sentAt: number, skewMs: number = 0): boolean {
  if (dateHeader === null) return false;
  const headerTime = Date.parse(dateHeader);
  if (Number.isNaN(headerTime)) return false;
  return sentAt - headerTime - skewMs > 60_000;
}

let onUnauthorized: () => void = () => {};
export function setUnauthorizedHandler(fn: () => void): void {
  onUnauthorized = fn;
}

/**
 * Who the queue currently belongs to. The outbox is one IndexedDB per origin, shared by every
 * account that signs in on the device, and a queued write deliberately outlives the session
 * that made it -- so on a shared device the next person to log in would otherwise replay
 * another user's writes under their own session (the server refuses them on ownership, and
 * `replay` then parks them permanently dead), and see their titles in the pending and failed
 * lists. Set from `../stores/session.ts` on every path that establishes or ends a session.
 */
let currentUserId: number | null = null;
export function setOutboxUser(id: number | null): void {
  currentUserId = id;
}

/**
 * Whether a server has actually confirmed the session the outbox user comes from. In offline
 * mode (see `../stores/session.ts`) the user is the last one remembered on this device, not one
 * the server has vouched for: their writes may queue, but sending them under whatever cookie is
 * there now could replay them as someone else. Injected rather than imported so `api.ts` keeps
 * not depending on the session module. Defaults to "confirmed" so code that sets an outbox user
 * directly (the outbox tests) behaves as before.
 */
let sessionConfirmed: () => boolean = () => true;
export function setOutboxSendGate(fn: () => boolean): void {
  sessionConfirmed = fn;
}

/** The outbox half (`api-outbox.ts`) reads these two through functions: a `let` cannot be
 *  shared across modules, and the values change at sign-in and sign-out. */
export function outboxUser(): number | null { return currentUserId; }
export function sendGateOpen(): boolean { return sessionConfirmed(); }

/** An op belongs to the session in front of us unless it is demonstrably someone else's. A
 *  record queued before `userId` existed, or one read while no user is known, counts as ours --
 *  the alternative is stranding a write nobody can ever see.
 *
 *  For DISPLAY only. Sending is stricter: see the guard at the top of `doFlushOutbox`. */
export function isOurs(op: QueuedOp): boolean {
  return op.userId === undefined || currentUserId === null || op.userId === currentUserId;
}

/**
 * `sentAt`/`gen` are optional only so this stays callable without them (there is no response to
 * have come from a cache before one exists); every real call site below passes both, captured
 * together at the moment the request went out. Recorded here, the one place every fetch
 * wrapper's response passes through, rather than in each of them, so every path is tracked the
 * same way regardless of which wrapper fetched it.
 */
async function handle<T>(res: Response, path: string, sentAt?: number, gen?: number, seq?: number): Promise<T> {
  if (sentAt !== undefined && res.ok) {
    const dateHeader = res.headers.get('date');
    // Calibrate the clock-skew estimate from a response that is never a cache hit (see
    // `clockSkewMs`) BEFORE judging this one -- harmless for an auth/settings response itself
    // (never cacheable, so never added to `staleKeys` regardless), but keeps every path's
    // judgement working off the freshest skew reading available. Not gated on `gen`: clock skew
    // is a property of the server, not of any one screen, so a late answer still calibrates it.
    if ((path.startsWith('/auth/') || path.startsWith('/settings')) && dateHeader !== null) {
      const t = Date.parse(dateHeader);
      const now = Date.now();
      const roundTripMs = now - sentAt;
      // The midpoint of the round trip, not `sentAt` itself: `sentAt` is when this device sent
      // the request, but the server wrote its `Date` header partway through the trip back, and
      // the midpoint is the best guess at "when" without a timestamp from the server itself. A
      // round trip so slow that this guess cannot be trusted (network congestion, a server that
      // took its time) is skipped entirely rather than risked -- reading its own latency as clock
      // skew once corrupted the estimate enough that every later FRESH response looked cached.
      if (!Number.isNaN(t) && roundTripMs <= MAX_CALIBRATION_ROUND_TRIP_MS) {
        clockSkewMs = (sentAt + now) / 2 - t;
      }
    }
    // Skip touching `staleKeys` for a response whose captured generation is stale (see
    // `routeGeneration`) -- a route change (or session end) has already happened since this
    // request was sent, so neither adding nor removing its path means anything for what is on
    // screen now. The same holds for a request whose query the screen has since replaced (see
    // `supersedeStale`).
    if ((gen === undefined || gen === routeGeneration) && (seq === undefined || !isSuperseded(path, seq))) {
      if (servedFromCache(dateHeader, sentAt, clockSkewMs)) staleKeys.add(path);
      else staleKeys.delete(path);
      servingSavedState.set(staleKeys.size > 0);
    }
  }
  if (res.status === 204) return undefined as T;
  const isJson = (res.headers.get('content-type') ?? '').includes('application/json');
  const body = isJson ? await res.json() : null;
  if (!res.ok) {
    if (res.status === 401 && !path.startsWith('/auth')) onUnauthorized();
    throw new ApiError(res.status, body?.error ?? 'error', body?.message ?? `HTTP ${res.status}`, body);
  }
  return body as T;
}

/**
 * `timeoutMs` is for the rare request that is *meant* to hold its connection open for a long
 * time -- today only the database switch, which answers once the whole copy has been made and
 * verified. `fetch` imposes no deadline of its own, so the default stays "as long as the
 * network allows"; a caller that passes one is choosing a ceiling far above the work, not a
 * normal request timeout. Setting it too low is the expensive mistake: the copy would keep
 * running server-side after the client gave up, and the operator would be told a migration
 * failed that in fact succeeded.
 */
export async function api<T = unknown>(method: string, path: string, body?: unknown, timeoutMs?: number): Promise<T> {
  const init: RequestInit = { method, credentials: 'same-origin', headers: {} };
  if (timeoutMs !== undefined) init.signal = AbortSignal.timeout(timeoutMs);
  if (body !== undefined) {
    init.headers = { 'content-type': 'application/json' };
    init.body = JSON.stringify(body);
  }
  const sentAt = Date.now();
  const gen = routeGeneration;
  const seq = ++requestSeq;
  const res = await fetch(`/api${path}`, init);
  return handle<T>(res, path, sentAt, gen, seq);
}

/**
 * A GET whose response is a page of a longer list. `total` comes from `X-Total-Count` and
 * counts everything matching the filters, not just this page, so the caller knows whether
 * there is more to fetch.
 */
export async function apiPage<T = unknown>(path: string): Promise<{ items: T[]; total: number }> {
  const sentAt = Date.now();
  const gen = routeGeneration;
  const seq = ++requestSeq;
  const res = await fetch(`/api${path}`, { method: 'GET', credentials: 'same-origin' });
  const items = await handle<T[]>(res, path, sentAt, gen, seq);
  const header = res.headers.get('x-total-count');
  const total = header === null ? items.length : Number(header);
  return { items, total: Number.isFinite(total) ? total : items.length };
}

export async function upload<T = unknown>(path: string, form: FormData): Promise<T> {
  const sentAt = Date.now();
  const gen = routeGeneration;
  const seq = ++requestSeq;
  const res = await fetch(`/api${path}`, { method: 'POST', credentials: 'same-origin', body: form });
  return handle<T>(res, path, sentAt, gen, seq);
}

export async function uploadRaw<T = unknown>(path: string, blob: Blob, contentType: string): Promise<T> {
  const sentAt = Date.now();
  const gen = routeGeneration;
  const seq = ++requestSeq;
  const res = await fetch(`/api${path}`, { method: 'POST', credentials: 'same-origin', headers: { 'content-type': contentType }, body: blob });
  return handle<T>(res, path, sentAt, gen, seq);
}

export function fileUrl(fileId: number, thumb = false): string {
  return `/api/files/${fileId}${thumb ? '/thumb' : ''}`;
}

export * from './api-outbox';
