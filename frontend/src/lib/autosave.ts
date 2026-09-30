/** What `autosave` hands the save function next to the value. `keepalive` is set on every send:
 *  the save function passes it on to `api(…, { keepalive })` so the browser finishes the request
 *  even when the page is left while it is out -- a plain fetch still running at unload is
 *  cancelled, and the setting with it. Bodies are a few hundred bytes and each saver has at most
 *  one request out, far below the browser's 64 KB keepalive budget. */
export type SaveOptions = { keepalive: true };

/**
 * Saves a setting a moment after the last change, one request at a time, and only ever the
 * newest value: two quick changes are one request, and a change made while a request is out is
 * sent when it returns, so an older answer can never land after a newer one. `onsaved` runs when
 * the newest value is on the server; `onerror` when sending it failed.
 *
 * A failure is shown inline next to its field (`role="alert"`), never as a toast, together with a
 * "Try again" button that calls `retry()`: a text field's `change` event does not fire again for
 * an unchanged value, so without it the only way to resend would be to edit the field.
 *
 * `flush` sends a waiting value now -- a page calls it when it is left (component destroy, and
 * `pagehide`). Every send carries `keepalive`, so one already out when the page goes is finished
 * too. A failure while leaving is lost: `onerror` may still run, but the page that would show it
 * is gone, and nothing retries it.
 */
export function autosave<T>(
  save: (value: T, opts?: SaveOptions) => Promise<void>,
  opts: { delay?: number; onsaved?: () => void; onerror?: (e: unknown) => void } = {},
): { push(value: T): void; flush(): Promise<void>; retry(): void } {
  const delay = opts.delay ?? 300;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending: { value: T } | null = null;
  /** The newest value pushed, sent or not: what `retry` sends again. */
  let latest: { value: T } | null = null;
  let running: Promise<void> | null = null;

  async function run(): Promise<void> {
    while (pending) {
      const { value } = pending;
      pending = null;
      try {
        await save(value, { keepalive: true });
        if (!pending) opts.onsaved?.();
      } catch (e) {
        if (!pending) opts.onerror?.(e);
      }
    }
    running = null;
  }
  function start(): void {
    clearTimeout(timer);
    timer = undefined;
    if (!running) running = run();
  }

  return {
    push(value: T) {
      pending = latest = { value };
      clearTimeout(timer);
      timer = setTimeout(start, delay);
    },
    flush() {
      if (timer !== undefined) start();
      return running ?? Promise.resolve();
    },
    retry() {
      if (!latest) return;
      pending = latest;
      start();
    },
  };
}
