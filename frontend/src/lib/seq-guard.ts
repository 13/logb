/**
 * A load-sequence guard: only the newest of several overlapping loads may commit its result.
 *
 * A view that loads on mount, on a prop change and after every outbox flush can have two loads
 * of the same thing in flight at once, and without this whichever answer arrives LAST wins --
 * not whichever request started last. `invalidate` makes every load in flight stale at once,
 * for a view whose component instance is reused across objects (`ObjectDetail` and its tabs):
 * an answer about the object just left must not land on the one navigated to.
 */
export function createSeq(): { next: () => number; current: (token: number) => boolean; invalidate: () => void } {
  let n = 0;
  return {
    next: () => ++n,
    current: (token) => token === n,
    invalidate: () => { n++; },
  };
}
