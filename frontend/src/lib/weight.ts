import type { QueuedOp } from './outbox';
import type { MemObject, WeightUnit } from './types';
const GRAMS_PER_LB = 453.59237;
export interface WeightPoint { id: number; date: string; created_at: string; weight_grams: number; pending?: boolean }
export function parseWeight(text: string, unit: WeightUnit): number {
  const normalized = text.trim().replace(',', '.');
  if (!/^\d+(?:\.\d+)?$/.test(normalized)) return NaN;
  const grams = Math.round(Number(normalized) * (unit === 'lb' ? GRAMS_PER_LB : 1000));
  return Number.isSafeInteger(grams) && grams > 0 && grams <= 1_000_000_000 ? grams : NaN;
}
export function weightValue(grams: number, unit: WeightUnit): number { return grams / (unit === 'lb' ? GRAMS_PER_LB : 1000); }
export function weightInput(grams: number | null | undefined, unit: WeightUnit): string {
  return grams == null ? '' : String(Number(weightValue(grams, unit).toFixed(3)));
}
/** Convert an in-progress weight field when its display unit changes. */
export function changeWeightUnitValue(value: string, from: WeightUnit, to: WeightUnit): string {
  const grams = parseWeight(value, from);
  return Number.isFinite(grams) ? weightInput(grams, to) : value;
}
export function formatWeight(grams: number, unit: WeightUnit, locale: string): string {
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 2 }).format(weightValue(grams, unit))} ${unit}`;
}
export function orderWeights(points: WeightPoint[]): WeightPoint[] {
  return [...points].sort((a,b) => b.date.localeCompare(a.date) || (Date.parse(b.created_at) - Date.parse(a.created_at)) || b.id-a.id);
}
/** Capabilities determine which single action is useful on an object's dashboard card. */
export function quickLogPath(object: Pick<MemObject, 'id' | 'type' | 'counter_unit' | 'fuel_unit' | 'resource_unit' | 'resource_kind'>): string {
  const root = `/objects/${object.id}`;
  if (object.type === 'body') return `${root}/activities/new?category=weight`;
  if (object.resource_unit || object.fuel_unit) return `${root}/activities/new?category=${object.resource_kind ? 'usage' : 'fuel'}`;
  if (object.counter_unit) return `${root}/reading`;
  return `${root}/activities/new`;
}

/** Overlay queued edits and creates without changing the canonical server history. */
export function withPendingWeights(history: WeightPoint[], ops: QueuedOp[]): WeightPoint[] {
  const points = new Map(history.map(p => [p.id, p]));
  let temporaryId = -ops.length;
  for (const op of ops) {
    const id = op.kind === 'activity.create' ? temporaryId++ : Number(op.path.split('/').at(-1));
    const previous = points.get(id);
    const grams = op.body.weight_grams;
    if (op.body.category !== 'weight' || typeof grams !== 'number' || grams <= 0) { points.delete(id); continue; }
    points.set(id, { id, date: String(op.body.date), weight_grams: grams,
      created_at: previous?.created_at ?? new Date(op.queued_at ?? 0).toISOString(), pending: true });
  }
  return orderWeights([...points.values()]);
}
