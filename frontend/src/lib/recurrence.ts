/** Calendar-only arithmetic, independent of daylight saving and browser timezone. */
export function previewDates(schedule: string | null, start: string | null, count = 3): string[] {
  if (!schedule) return [];
  const now = new Date();
  const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
  const date = new Date(`${start || today}T12:00:00Z`);
  if (!Number.isFinite(date.getTime())) return [];
  const [kind, a, b] = schedule.split(':');
  const result: string[] = [];
  for (let i = 0; i < 1500 && result.length < count; i++) {
    const y = date.getUTCFullYear(), m = date.getUTCMonth(), d = date.getUTCDate();
    const last = new Date(Date.UTC(y, m + 1, 0)).getUTCDate();
    const match = kind === 'daily'
      || (kind === 'weekly' && (date.getUTCDay() || 7) === Number(a))
      || (kind === 'monthly' && d === Math.min(a === 'last' ? 31 : Number(a), last))
      || (kind === 'yearly' && m + 1 === Number(a) && d === Math.min(Number(b), last));
    if (match) result.push(date.toISOString().slice(0, 10));
    date.setUTCDate(d + 1);
  }
  return result;
}
