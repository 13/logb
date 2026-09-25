/**
 * `ApiError` and the offline/rejection classification live in their own module, separate from
 * `./api.ts`, purely so `./outbox.ts` can import the classification without creating an
 * import cycle (`api.ts` already imports `outbox.ts`). `api.ts` re-exports both so nothing
 * else needs to know this file exists.
 */

/** Thrown by `api()` when the server answered but refused the request. */
export class ApiError extends Error {
  /** `body` is the whole JSON error, for the few answers that carry more than a sentence -- e.g.
   *  a 409 `in_use` with the `count` of objects still using a type. */
  constructor(public status: number, public code: string, message: string, public body: Record<string, unknown> | null = null) {
    super(message);
  }
}

/**
 * True when the SERVER refused this request outright — an `ApiError` carrying a 4xx status,
 * meaning the request reached the server and was rejected on its merits (bad input, not
 * found, forbidden, a stale precondition, ...).
 *
 * Every other failure — a dropped connection, an aborted fetch, a malformed (non-JSON)
 * response that throws a `SyntaxError` inside `handle()`, a 5xx — means we simply don't know
 * whether the server saw and applied the write, so none of them may be treated as a rejection.
 * This is why the check keys on what the server actually said rather than on how the failure
 * happened to be *typed* (`instanceof TypeError`, `navigator.onLine`, ...): a `SyntaxError`
 * from a malformed JSON body is exactly as "unknown" as a network drop, and `navigator.onLine`
 * reading false for a moment must never override a 4xx the server has already sent back.
 *
 * Treating every non-rejection as "unsendable, so queue it" (see `createQueued` in `./api.ts`)
 * is safe precisely because every queued op carries its `client_op_id`: if the server did in
 * fact apply the write before the failure occurred, replaying it later resolves to the same
 * row instead of duplicating it. The one failure this reasoning does NOT cover is the local
 * queue write itself failing — there is nothing to replay against, so that must surface to the
 * caller instead of being swallowed.
 */
export function isRejection(e: unknown): boolean {
  return e instanceof ApiError && e.status >= 400 && e.status < 500;
}

/**
 * True for the one 4xx that says nothing about the request itself: the caller is not
 * authenticated. Unlike every other rejection it is not permanent — logging back in makes the
 * very same request succeed — so a queued write must survive it rather than being parked dead
 * (see `replay` in `./outbox.ts`).
 *
 * 401 only, deliberately not 403: a 403 means the server knows who is asking and still refuses,
 * which no amount of retrying will change, so it stays an ordinary permanent rejection.
 *
 * This is NOT folded into `isRejection`, which several read paths use to decide whether they
 * may fall back to a session cache: there, a 401 or 403 is precisely the answer that must NOT
 * be served from cache (see the `onMount` catch in `../routes/ActivityForm.svelte`).
 */
export function isUnauthenticated(e: unknown): boolean {
  return e instanceof ApiError && e.status === 401;
}

/** The translate function, as the `t` store hands it out (`$t` in a component). */
export type Translate = (key: string, vars?: Record<string, string | number>) => string;

/** A failure whose `message` is an i18n key and which needs placeholders filled in ("Line
 *  {line}: …"). Thrown by pure modules that have no `t` of their own; `errorMessage` says it. */
export class I18nError extends Error {
  constructor(key: string, public vars?: Record<string, string | number>) {
    super(key);
  }
}

/**
 * Stable server codes (`src/error.rs`) whose MESSAGE says nothing the code does not: for these
 * the reader gets a sentence in their own language. Codes that carry the detail in the message
 * instead -- `bad_request`, `conflict`, and `unavailable` in general -- are deliberately absent,
 * so the server's own (more specific) sentence survives.
 */
const CODE_KEYS: Record<string, string> = {
  unauthorized: 'error.unauthorized',
  forbidden: 'error.forbidden',
  not_found: 'error.not-found',
  gone: 'error.gone',
  too_large: 'error.too-large',
  too_many_requests: 'error.too-many-requests',
  internal: 'error.internal',
  name_taken: 'types.error.name_taken',
  name_invalid: 'types.error.name_invalid',
  icon_invalid: 'types.error.icon_invalid',
  categories_invalid: 'types.error.categories_invalid',
  unit_invalid: 'types.error.unit_invalid',
  wrong_password: 'settings.wrong-password',
};

/** The one `unavailable` answer with a fixed sentence: the writer (or the database) was busy and
 *  the request never ran. Every other `unavailable` names its own cause ("save a Telegram bot
 *  token first") and keeps it. */
const BUSY_MESSAGE = 'the database is busy, please retry';

/** What each browser's `fetch` throws (a `TypeError`) when the request never got an answer:
 *  Chromium's "Failed to fetch", Firefox's "NetworkError when attempting to fetch resource.",
 *  Safari's "Load failed". Any other `TypeError` is a bug, and must not be reported as "no
 *  connection". */
const NETWORK_FAILURE = /failed to fetch|networkerror|load failed|network request failed/i;

/** Shaped like one of our own i18n keys (`outbox.queue-failed`, `types.error.name_taken`): a few
 *  lower-case dot-separated words and no spaces -- never a sentence. */
const KEY_SHAPE = /^[a-z][a-z0-9-]*(\.[a-z0-9_-]+)+$/;

function browserOffline(): boolean {
  return typeof navigator !== 'undefined' && navigator.onLine === false;
}

/**
 * The one sentence to show a reader for a failed request or action, already translated.
 *
 * - The server answered (`ApiError`): a known stable code becomes its key; anything else keeps
 *   the server's own sentence. What the server said always wins over `navigator.onLine`.
 * - It never answered (a `fetch` network failure, or anything thrown while the browser says it
 *   is offline): `error.offline`.
 * - A message that is one of our own keys (thrown by e.g. `createObjectQueued`) is translated.
 * - Anything else is shown as it is, and an empty one falls back to `error.generic`.
 */
export function errorMessage(e: unknown, t: Translate): string {
  if (e instanceof ApiError) {
    if (e.code === 'unavailable' && e.message === BUSY_MESSAGE) return t('error.busy');
    const key = CODE_KEYS[e.code];
    return key ? t(key) : e.message || t('error.generic');
  }
  if (e instanceof I18nError) return t(e.message, e.vars);
  if (browserOffline()) return t('error.offline');
  return messageText(e instanceof Error ? e.message : typeof e === 'string' ? e : '', t);
}

/**
 * `errorMessage` for a failure of which only the message survives (a dead outbox op's
 * `lastError`): a network failure's text says "no connection", one of our own keys is
 * translated, anything else is shown as it is. Deliberately blind to `navigator.onLine`: a
 * failure recorded earlier says nothing about the connection now.
 */
export function messageText(message: string, t: Translate): string {
  if (!message) return t('error.generic');
  if (NETWORK_FAILURE.test(message)) return t('error.offline');
  return KEY_SHAPE.test(message) ? t(message) : message;
}
