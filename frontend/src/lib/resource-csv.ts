import { parseMoney, parseQuantity } from './format';
import type { ActivityInput, MeasurementMode } from './types';

function row(line: string, separator: string): string[] {
  const values: string[] = []; let value = ''; let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (ch === '"' && quoted && line[i + 1] === '"') { value += '"'; i++; }
    else if (ch === '"') quoted = !quoted;
    else if (ch === separator && !quoted) { values.push(value.trim()); value = ''; }
    else value += ch;
  }
  values.push(value.trim()); return values;
}

export function parseResourceCsv(text: string, mode: MeasurementMode): ActivityInput[] {
  const lines = text.replace(/^\uFEFF/, '').split(/\r?\n/).filter((line) => line.trim());
  if (lines.length < 2) throw new Error('CSV needs a header and at least one row.');
  const separator = (lines[0].match(/;/g)?.length ?? 0) > (lines[0].match(/,/g)?.length ?? 0) ? ';' : ',';
  const headers = row(lines[0], separator).map((h) => h.toLowerCase().replace(/\s+/g, '_'));
  const at = (...names: string[]) => names.map((name) => headers.indexOf(name)).find((i) => i >= 0) ?? -1;
  const dateAt = at('date'); const valueAt = mode === 'meter' ? at('reading', 'meter_reading') : at('amount', 'usage', 'value');
  if (dateAt < 0 || valueAt < 0) throw new Error(mode === 'meter' ? 'CSV needs date and reading columns.' : 'CSV needs date and amount columns.');
  return lines.slice(1).map((line, index) => {
    const values = row(line, separator); const date = values[dateAt] ?? '';
    if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) throw new Error(`Line ${index + 2}: date must be YYYY-MM-DD.`);
    const amount = parseQuantity(values[valueAt] ?? '');
    if (amount === null || Number.isNaN(amount) || amount < 0) throw new Error(`Line ${index + 2}: invalid value.`);
    const costAt = at('cost'); const startAt = at('period_start'); const endAt = at('period_end'); const estimatedAt = at('estimated');
    const base: ActivityInput = { date, category: 'usage', title: '', notes: '', counter_value: null,
      cost_cents: costAt < 0 ? null : parseMoney(values[costAt] ?? ''), quantity_milli: mode === 'meter' ? null : amount,
      meter_reading_milli: mode === 'meter' ? amount : null, period_start: startAt < 0 ? null : values[startAt] || null,
      period_end: endAt < 0 ? null : values[endAt] || null, estimated: estimatedAt >= 0 && /^(1|true|yes)$/i.test(values[estimatedAt] ?? '') ? 1 : 0,
      meter_reset: 0, tags: [] };
    return base;
  });
}
