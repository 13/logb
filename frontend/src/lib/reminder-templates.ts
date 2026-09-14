import { addMonthsIso } from './reading';
import { emptyReminder, readingReminder } from './reminder-form';
import type { CounterUnit, ObjectType, ReminderInput } from './types';

/**
 * Reminders a new object can start with. Offered, never created unasked: the object form lists
 * the ones for its type as checkboxes, all unticked.
 *
 * The intervals are common rules of thumb, not a manufacturer's schedule, and the form says each
 * can be changed afterwards. A distance or hours step is given per unit, since 15,000 km and
 * 10,000 miles are the same advice written for two dashboards.
 */
export interface ReminderTemplate {
  id: string;
  /** i18n key of the reminder's title. */
  title: string;
  months?: number;
  counter?: Partial<Record<'km' | 'mi' | 'h', number>>;
  /** A reading reminder: log the counter every month. */
  reading?: boolean;
}

const READING: ReminderTemplate = { id: 'reading', title: 'reading.reminder-title', reading: true };

const TEMPLATES: Record<ObjectType, ReminderTemplate[]> = {
  car: [
    { id: 'oil', title: 'template.oil', months: 12, counter: { km: 15_000, mi: 10_000 } },
    { id: 'inspection', title: 'template.inspection', months: 24 },
    { id: 'tyres', title: 'template.tyres', months: 6 },
    READING,
  ],
  motorcycle: [
    { id: 'service', title: 'template.service', months: 12, counter: { km: 6_000, mi: 4_000 } },
    { id: 'chain', title: 'template.chain', counter: { km: 1_000, mi: 600 } },
    { id: 'inspection', title: 'template.inspection', months: 24 },
    READING,
  ],
  e_bike: [
    { id: 'service', title: 'template.service', months: 12, counter: { km: 2_000, mi: 1_250 } },
    { id: 'chain', title: 'template.chain', months: 3 },
    READING,
  ],
  bike: [
    { id: 'service', title: 'template.service', months: 12 },
    { id: 'chain', title: 'template.chain', months: 3 },
  ],
  home: [
    { id: 'smoke', title: 'template.smoke', months: 12 },
    { id: 'heating', title: 'template.heating', months: 12 },
  ],
  appliance: [
    { id: 'descale', title: 'template.descale', months: 3 },
    { id: 'filter', title: 'template.filter', months: 6 },
  ],
  tool: [
    { id: 'service', title: 'template.service', months: 12, counter: { h: 50 } },
    READING,
  ],
  body: [
    { id: 'checkup', title: 'template.checkup', months: 12 },
    { id: 'dentist', title: 'template.dentist', months: 6 },
  ],
  other: [],
};

/** The counter step a template has for `unit`, if any. */
export function counterStep(t: ReminderTemplate, unit: CounterUnit): number | null {
  return unit ? t.counter?.[unit] ?? null : null;
}

/** What the form offers for this type and counter: a template that only makes sense with a
 *  counter -- a reading, or one due by distance alone -- needs the object to have one. */
export function templatesFor(type: ObjectType, unit: CounterUnit): ReminderTemplate[] {
  return TEMPLATES[type].filter((t) => {
    if (t.reading) return unit !== null;
    return t.months !== undefined || counterStep(t, unit) !== null;
  });
}

/**
 * The reminder a ticked template becomes.
 *
 * A distance-based reminder needs to know where the counter is now -- "due at 15,000 km" means
 * nothing on a car already at 80,000 -- so its counter half is only set when `currentReading` is
 * known. Without it, a template with months falls back to the date alone, and one with nothing
 * but a distance step is skipped (`null`) rather than created wrong.
 */
export function templateInput(
  t: ReminderTemplate,
  opts: { title: string; unit: CounterUnit; currentReading: number | null; today: string },
): ReminderInput | null {
  if (t.reading) return opts.unit ? readingReminder(opts.title, addMonthsIso(opts.today, 1)) : null;
  const step = counterStep(t, opts.unit);
  const dueCounter = step !== null && opts.currentReading !== null ? opts.currentReading + step : null;
  const dueDate = t.months !== undefined ? addMonthsIso(opts.today, t.months) : null;
  if (dueDate === null && dueCounter === null) return null;
  return {
    ...emptyReminder(),
    title: opts.title,
    due_date: dueDate,
    repeat_months: t.months ?? null,
    due_counter: dueCounter,
    repeat_counter: dueCounter !== null ? step : null,
  };
}
