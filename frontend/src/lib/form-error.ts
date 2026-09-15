/** A translator as the `t` store hands it out. */
type Translate = (key: string, vars?: Record<string, string | number>) => string;

/**
 * `validateActivity`'s trip keys each get their own sentence (the spec's own wording, e.g. "End
 * must not be below start") instead of the generic "Check <field>" below -- those four keys name
 * what is wrong, not just which field to look at, so wrapping them in the generic sentence would
 * lose that.
 */
const TRIP_MESSAGES: Record<string, string> = {
  'trip.start': 'trip.error-start',
  'trip.end': 'trip.error-end',
  'trip.battery': 'trip.error-battery',
  'trip.duration': 'trip.error-duration',
};

/**
 * The message for a field a form's `validate*` function rejected.
 *
 * Those functions answer with the i18n key of the field's *label*, so a form that showed
 * `$t(key)` directly put up a bare "Title" in red -- a word, not a message. This wraps the label
 * in a sentence that says what to do about it.
 */
export function fieldError(key: string, t: Translate): string {
  const tripMessage = TRIP_MESSAGES[key];
  if (tripMessage) return t(tripMessage);
  return t('form.check-field', { field: t(key) });
}
