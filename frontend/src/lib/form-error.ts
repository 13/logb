/** A translator as the `t` store hands it out. */
type Translate = (key: string, vars?: Record<string, string | number>) => string;

/**
 * The message for a field a form's `validate*` function rejected.
 *
 * Those functions answer with the i18n key of the field's *label*, so a form that showed
 * `$t(key)` directly put up a bare "Title" in red -- a word, not a message. This wraps the label
 * in a sentence that says what to do about it.
 */
export function fieldError(key: string, t: Translate): string {
  return t('form.check-field', { field: t(key) });
}
