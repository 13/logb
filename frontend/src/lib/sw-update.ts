import { writable } from 'svelte/store';

/**
 * Service worker updates, in prompt mode (`registerType: 'prompt'` in vite.config.ts).
 *
 * Auto-update reloaded the page the moment a new build's worker took over -- in the middle of
 * a form, throwing away whatever was typed. Now a waiting worker is only OFFERED
 * (`UpdateBanner.svelte`), and the reload happens when the user asks for it.
 *
 * One case still reaches a tab that did not ask: the user accepts in ANOTHER tab, the new worker
 * activates for every tab of the origin, and this one is now controlled by a worker whose
 * precache no longer matches the code it is running. That tab gets the same offer instead of
 * the plugin's default `location.reload()`.
 */

export const HOUR = 60 * 60 * 1000;

/** Whether the banner is up. */
export const updateReady = writable(false);

let apply: (() => Promise<void> | void) | null = null;
/** Set once THIS tab accepted the update, so the worker taking control reloads it. */
let requested = false;

/** A new worker is installed and waiting; `activate` makes it take over (and reload). */
export function offerUpdate(activate: () => Promise<void> | void): void {
  apply = activate;
  updateReady.set(true);
}

export function dismissUpdate(): void {
  updateReady.set(false);
}

export async function applyUpdate(): Promise<void> {
  requested = true;
  updateReady.set(false);
  await apply?.();
}

/** The new worker controls this tab now (`onNeedReload` of `registerSW`). */
export function needReload(reload: () => void = () => location.reload()): void {
  if (requested) { reload(); return; }
  offerUpdate(reload);
}

/**
 * A PWA left open for days never navigates, so the browser never re-checks for a new worker on
 * its own; this asks once an hour. Skipped while offline, where it can only fail, and a failed
 * check is not worth reporting -- the next one tries again.
 */
export function scheduleUpdateChecks(
  registration: { update: () => Promise<unknown> },
  timer: (fn: () => void, ms: number) => unknown = (fn, ms) => setInterval(fn, ms),
  isOnline: () => boolean = () => globalThis.navigator?.onLine !== false,
): void {
  timer(() => {
    if (!isOnline()) return;
    registration.update().catch(() => {});
  }, HOUR);
}

/** Test seam only. */
export function resetUpdateForTesting(): void {
  apply = null;
  requested = false;
  updateReady.set(false);
}
