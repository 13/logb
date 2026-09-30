/**
 * Saves a setting a moment after the last change, one request at a time, and only ever the
 * newest value: two quick changes are one request, and a change made while a request is out is
 * sent when it returns, so an older answer can never land after a newer one. `onsaved` runs when
 * the newest value is on the server; `onerror` when sending it failed. `flush` sends a waiting
 * value now -- a page calls it when it is left.
 */
export function autosave<T>(
  save: (value: T) => Promise<void>,
  opts: { delay?: number; onsaved?: () => void; onerror?: (e: unknown) => void } = {},
): { push(value: T): void; flush(): Promise<void> } {
  const delay = opts.delay ?? 300;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending: { value: T } | null = null;
  let running: Promise<void> | null = null;

  async function run(): Promise<void> {
    while (pending) {
      const { value } = pending;
      pending = null;
      try {
        await save(value);
        if (!pending) opts.onsaved?.();
      } catch (e) {
        if (!pending) opts.onerror?.(e);
      }
    }
    running = null;
  }
  function start(): void {
    timer = undefined;
    if (!running) running = run();
  }

  return {
    push(value: T) {
      pending = { value };
      clearTimeout(timer);
      timer = setTimeout(start, delay);
    },
    flush() {
      if (timer !== undefined) { clearTimeout(timer); start(); }
      return running ?? Promise.resolve();
    },
  };
}
