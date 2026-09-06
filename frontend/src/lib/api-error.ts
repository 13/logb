/**
 * `ApiError` and the offline/rejection classification live in their own module, separate from
 * `./api.ts`, purely so `./outbox.ts` can import the classification without creating an
 * import cycle (`api.ts` already imports `outbox.ts`). `api.ts` re-exports both so nothing
 * else needs to know this file exists.
 */

/** Thrown by `api()` when the server answered but refused the request. */
export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) {
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
