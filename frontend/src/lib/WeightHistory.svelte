<script lang="ts">
  import { api, onOutboxFlushed, pendingActivityOps } from './api';
  import { go } from './router';
  import { formatWeight, withPendingWeights, weightValue, type WeightPoint } from './weight';
  import { addMonthsIso } from './reading';
  import { fmtDate, todayIso } from './format';
  import { dateFormat } from '../stores/date-format';
  import { locale, t } from '../i18n';
  import type { QueuedOp } from './outbox';
  import type { WeightUnit } from './types';
  let { objectId, unit }: { objectId: number; unit: WeightUnit } = $props();
  let history = $state<WeightPoint[]>([]);
  let pending = $state<QueuedOp[]>([]);
  let loaded = $state(false);
  let failed = $state(false);
  let range = $state<1 | 3 | 0>(3);
  let selected = $state<number | null>(null);
  let sequence = 0;
  async function load() {
    const request = ++sequence;
    try {
      const rows = await api<WeightPoint[]>('GET', `/objects/${objectId}/weight`);
      if (request !== sequence) return;
      history = rows; failed = false; loaded = true;
    } catch {
      if (request !== sequence) return;
      failed = true; loaded = true;
    }
    const ops = await pendingActivityOps(objectId, history.map(p => p.id));
    if (request === sequence) pending = ops;
  }
  $effect(() => { objectId; history = []; pending = []; loaded = false; failed = false; selected = null; load(); return () => { sequence++; }; });
  $effect(() => onOutboxFlushed((_resolved, changed) => { if (changed) load(); }));
  // The history endpoint is independent of timeline filters/pagination. Pending entries are
  // added only for display and labelled; the server still owns reminders and canonical stats.
  const all = $derived(withPendingWeights(history, pending));
  const latest = $derived(all[0]);
  const previous = $derived(all[1]);
  const cutoff = $derived(range === 0 ? '' : addMonthsIso(todayIso(), -range));
  const visible = $derived(all.filter(p => p.date >= cutoff).reverse());
  const low = $derived(Math.min(...visible.map(p => p.weight_grams)));
  const high = $derived(Math.max(...visible.map(p => p.weight_grams)));
  const start = $derived(Date.parse(visible[0]?.date ?? '2000-01-01'));
  const end = $derived(Date.parse(visible.at(-1)?.date ?? '2000-01-01'));
  function x(p: WeightPoint) { return end === start ? 250 : 48 + 410 * (Date.parse(p.date) - start) / (end - start); }
  function y(p: WeightPoint) { return high === low ? 90 : 145 - 110 * (p.weight_grams - low) / (high - low); }
  const line = $derived(visible.map(p => `${x(p)},${y(p)}`).join(' '));
  const focused = $derived(visible.find(p => p.id === selected) ?? visible.at(-1));
</script>

<section class="weight-history" aria-label={$t('weight.history')}>
  <div class="heading">
    <h2>{$t('weight.history')}</h2>
    <button class="primary" onclick={() => go(`/objects/${objectId}/activities/new?category=weight`)}>{$t('weight.log')}</button>
  </div>
  {#if latest}
    <div class="summary">
      <div><span class="muted">{$t('weight.latest')}</span><strong class="tnum">{formatWeight(latest.weight_grams, unit, $locale)}</strong><span class="muted">{fmtDate(latest.date, $dateFormat)}{#if latest.pending} · {$t('weight.pending')}{/if}</span></div>
      {#if previous}<div><span class="muted">{$t('weight.change')}</span><strong class="tnum">{latest.weight_grams > previous.weight_grams ? '+' : ''}{formatWeight(latest.weight_grams - previous.weight_grams, unit, $locale)}</strong></div>{/if}
    </div>
  {:else if loaded && !failed}<p class="muted">{$t('weight.empty')}</p>
  {:else if !loaded}<p class="muted">{$t('nav.loading')}</p>{/if}
  {#if failed}<p role="status">{$t('weight.unavailable')}</p>{/if}
  {#if all.length > 0}
    <div class="ranges" aria-label={$t('weight.history')}>
      {#each [1, 3, 0] as months}<button class="ghost" aria-pressed={range === months} onclick={() => { range = months as 1 | 3 | 0; selected = null; }}>{$t(months === 1 ? 'weight.month' : months === 3 ? 'weight.three-months' : 'weight.all')}</button>{/each}
    </div>
    {#if visible.length}
      <svg viewBox="0 0 500 185" role="img" aria-label={$t('weight.history')}>
        <title>{$t('weight.history')} ({unit})</title>
        <text x="4" y="38">{weightValue(high, unit).toFixed(1)}</text>
        {#if high !== low}<text x="4" y="148">{weightValue(low, unit).toFixed(1)}</text>{/if}
        <polyline points={line} fill="none" stroke="currentColor" stroke-width="2" />
        {#each visible as point (point.id)}
          <circle cx={x(point)} cy={y(point)} r={focused?.id === point.id ? 6 : 4} fill="currentColor" />
        {/each}
        <text x="48" y="178">{fmtDate(visible[0].date, $dateFormat)}</text>
        {#if visible.length > 1}<text x="458" y="178" text-anchor="end">{fmtDate(visible[visible.length - 1].date, $dateFormat)}</text>{/if}
      </svg>
      <label for="weight-point-chooser" class="muted">{$t('weight.chart-hint')}</label>
      <select id="weight-point-chooser" value={focused?.id} onchange={(e) => selected = Number(e.currentTarget.value)}>
        {#each visible as point (point.id)}<option value={point.id}>{fmtDate(point.date, $dateFormat)} · {formatWeight(point.weight_grams, unit, $locale)}{point.pending ? ` · ${$t('weight.pending')}` : ''}</option>{/each}
      </select>
    {:else}<p class="muted">{$t('weight.no-range')}</p>{/if}
  {/if}
  <button class="ghost reminder" onclick={() => go(`/objects/${objectId}/reminders/new?kind=reading`)}>{$t('weight.reminder')}</button>
</section>

<style>
  .weight-history { margin-block: var(--space-3); padding: var(--space-3); border: 1px solid var(--border); border-radius: var(--radius-sm); }
  .heading, .summary, .ranges { display: flex; gap: var(--space-3); flex-wrap: wrap; align-items: center; }
  .heading { justify-content: space-between; } h2 { margin: 0; }
  .summary { margin-top: var(--space-3); } .summary > div { display: flex; flex-direction: column; gap: var(--space-1); }
  strong { font-size: var(--text-lg); } .ranges { margin-top: var(--space-3); gap: var(--space-1); }
  [aria-pressed="true"] { background: var(--surface-2); box-shadow: inset 0 -2px currentColor; }
  svg { display: block; width: 100%; max-height: 230px; } text { fill: var(--muted); font-size: var(--text-xs); }
  select { width: 100%; } .reminder { margin-top: var(--space-2); }
</style>
