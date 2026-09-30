import { writable } from 'svelte/store';

/** The one message a page's `Toaster` shows: "Saved", or what an action did. `id` is new with
 *  every message, so the same text twice is announced twice. */
export const toastMessage = writable<{ id: number; text: string } | null>(null);

let next = 0;
let timer: ReturnType<typeof setTimeout> | undefined;

/** Shows `text` for `ms`, replacing whatever was showing. Not for errors: an error stays on the
 *  page, next to what caused it, until it is fixed. */
export function toast(text: string, ms = 3000): void {
  clearTimeout(timer);
  const id = ++next;
  toastMessage.set({ id, text });
  timer = setTimeout(() => toastMessage.update((m) => (m?.id === id ? null : m)), ms);
}
