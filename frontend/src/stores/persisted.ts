import { writable, type Writable } from 'svelte/store';

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

export function persisted<T>(key: string, initial: T): Writable<T> {
  let start = initial;
  try {
    const raw = globalThis.localStorage?.getItem(key);
    if (raw) {
      const env = JSON.parse(raw);
      if (env && env.v === 1) {
        const data = env.data as T;
        start = isPlainObject(initial) && isPlainObject(data) ? ({ ...initial, ...data } as T) : data;
      }
    }
  } catch { /* corrupt storage → initial */ }
  const store = writable<T>(start);
  store.subscribe((data) => {
    try { globalThis.localStorage?.setItem(key, JSON.stringify({ v: 1, data })); } catch { /* ignore */ }
  });
  return store;
}
