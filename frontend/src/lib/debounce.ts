/**
 * Hands each value pushed to it to `apply` once nothing newer has arrived for `ms` -- except a
 * value `immediate` accepts, which is applied at once (and drops any value still waiting). For a
 * filter typed into a box: the list is not re-filtered on every keystroke, but clearing the box
 * shows everything again without a pause.
 */
export function debouncer<T>(apply: (value: T) => void, ms: number, immediate: (value: T) => boolean = () => false) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const cancel = () => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };
  return {
    push(value: T): void {
      cancel();
      if (immediate(value)) { apply(value); return; }
      timer = setTimeout(() => { timer = null; apply(value); }, ms);
    },
    cancel,
  };
}
