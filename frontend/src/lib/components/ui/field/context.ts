import { getContext, setContext } from 'svelte';

/** What a control inside `Field` needs from it: its id (the label's `for`), what describes it
 *  (hint, warning, error), whether it is in error, and the unit drawn inside it. Read through
 *  getters, so a control follows an error that comes and goes. */
export interface FieldContext {
  readonly id: string;
  readonly describedBy: string | undefined;
  readonly invalid: boolean;
  readonly unit: string | null;
}

const KEY = Symbol('field');

export function setFieldContext(ctx: FieldContext): void {
  setContext(KEY, ctx);
}

export function getFieldContext(): FieldContext | undefined {
  return getContext<FieldContext | undefined>(KEY);
}
