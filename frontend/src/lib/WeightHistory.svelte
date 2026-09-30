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
  import { Button } from '$lib/components/ui/button/index.js';
  import { Field } from '$lib/components/ui/field/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  let { objectId, unit }: { objectId: number; unit: WeightUnit } = $props();
  let history = $state<WeightPoint[]>([]);
  let pending = $state<QueuedOp[]>([]);
  let loaded = $state(false);
  let failed = $state(false);
  let summary = $state<{ minimum_grams: number | null; maximum_grams: number | null; average_grams: number | null } | null>(null);
  let range = $state<1 | 3 | 0>(3);
  let selected = $state<number | null>(null);
  let sequence = 0;
  async function load() {
    const request = ++sequence;
    try {
      // Both at once; the summary is optional, the history is not.
      const [rows, totals] = await Promise.all([
        api<WeightPoint[]>('GET', `/objects/${objectId}/weight`),
        api<typeof summary>('GET', `/objects/${objectId}/weight/summary`).catch(() => null),
      ]);
      if (request !== sequence) return;
      history = rows; failed = false; loaded = true;
      summary = totals;
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
  const label = 'text-sm text-muted-foreground';
  const figure = 'text-lg font-semibold text-foreground tabular-nums';
  const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring';
</script>

<section aria-label={$t('weight.history')} class="my-3 flex flex-col gap-3 rounded-lg border border-border bg-card p-3 shadow-xs">
  <div class="flex flex-wrap items-center justify-between gap-3">
    <h2 class="m-0 text-base font-semibold text-foreground">{$t('weight.history')}</h2>
    <Button class="min-h-11" onclick={() => go(`/objects/${objectId}/activities/new?category=weight`)}>{$t('weight.log')}</Button>
  </div>
  {#if latest}
    <div class="flex flex-wrap gap-x-6 gap-y-2">
      <div class="flex flex-col gap-0.5">
        <span class={label}>{$t('weight.latest')}</span>
        <strong class={figure}>{formatWeight(latest.weight_grams, unit, $locale)}</strong>
        <span class={label}>{fmtDate(latest.date, $dateFormat)}{#if latest.pending} · {$t('weight.pending')}{/if}</span>
      </div>
      {#if previous}
        <div class="flex flex-col gap-0.5">
          <span class={label}>{$t('weight.change')}</span>
          <strong class={figure}>{latest.weight_grams > previous.weight_grams ? '+' : ''}{formatWeight(latest.weight_grams - previous.weight_grams, unit, $locale)}</strong>
        </div>
      {/if}
      {#if summary?.average_grams != null}
        <div class="flex flex-col gap-0.5">
          <span class={label}>{$t('weight.average')}</span>
          <strong class={figure}>{formatWeight(summary.average_grams, unit, $locale)}</strong>
        </div>
      {/if}
    </div>
  {:else if loaded && !failed}
    <p class={`m-0 ${label}`}>{$t('weight.empty')}</p>
  {:else if !loaded}
    <p class={`m-0 ${label}`}>{$t('nav.loading')}</p>
  {/if}
  {#if failed}<p role="status" class="m-0 text-sm text-foreground">{$t('weight.unavailable')}</p>{/if}
  {#if all.length > 0}
    <!-- The dashboard's segmented control: the chosen range on a card-coloured segment. -->
    <div role="group" aria-label={$t('weight.history')} class="flex w-fit gap-1 rounded-md bg-muted p-1">
      {#each [1, 3, 0] as months (months)}
        <button type="button" data-slot="weight-range" aria-pressed={range === months}
                onclick={() => { range = months as 1 | 3 | 0; selected = null; }}
                class={['min-h-11 cursor-pointer rounded px-3 text-sm font-medium', focus, range === months ? 'bg-card text-foreground shadow-xs' : 'text-muted-foreground hover:text-foreground']}>
          {$t(months === 1 ? 'weight.month' : months === 3 ? 'weight.three-months' : 'weight.all')}
        </button>
      {/each}
    </div>
    {#if visible.length}
      <svg viewBox="0 0 500 185" role="img" aria-label={$t('weight.history')} class="block max-h-[230px] w-full text-brand-ink">
        <title>{$t('weight.history')} ({unit})</title>
        <text x="4" y="38" class="fill-muted-foreground text-xs">{weightValue(high, unit).toFixed(1)}</text>
        {#if high !== low}<text x="4" y="148" class="fill-muted-foreground text-xs">{weightValue(low, unit).toFixed(1)}</text>{/if}
        <polyline points={line} fill="none" stroke="currentColor" stroke-width="2" />
        {#each visible as point (point.id)}
          <circle cx={x(point)} cy={y(point)} r={focused?.id === point.id ? 6 : 4} fill="currentColor" />
        {/each}
        <text x="48" y="178" class="fill-muted-foreground text-xs">{fmtDate(visible[0].date, $dateFormat)}</text>
        {#if visible.length > 1}<text x="458" y="178" text-anchor="end" class="fill-muted-foreground text-xs">{fmtDate(visible[visible.length - 1].date, $dateFormat)}</text>{/if}
      </svg>
      <Field id="weight-point-chooser" label={$t('weight.chart-hint')}>
        <NativeSelect bind:value={() => focused?.id, (v) => (selected = Number(v))}>
          {#each visible as point (point.id)}<option value={point.id}>{fmtDate(point.date, $dateFormat)} · {formatWeight(point.weight_grams, unit, $locale)}{point.pending ? ` · ${$t('weight.pending')}` : ''}</option>{/each}
        </NativeSelect>
      </Field>
      <ol aria-label={$t('weight.history')} class="m-0 flex list-none flex-col gap-1 p-0">
        {#each visible as point (point.id)}
          <li>
            <button type="button" data-slot="weight-point" aria-label={$t('weight.chart-point')} onclick={() => (selected = point.id)}
                    class={['flex min-h-11 w-full cursor-pointer items-center justify-between gap-2 rounded-md px-3 text-left text-sm text-foreground hover:bg-accent', focus, focused?.id === point.id && 'outline-2 outline-solid outline-brand-ink']}>
              <span>{fmtDate(point.date, $dateFormat)}</span>
              <span><strong class="font-semibold tabular-nums">{formatWeight(point.weight_grams, unit, $locale)}</strong>{#if point.pending}<span class="text-muted-foreground"> · {$t('weight.pending')}</span>{/if}</span>
            </button>
          </li>
        {/each}
      </ol>
    {:else}
      <p class={`m-0 ${label}`}>{$t('weight.no-range')}</p>
    {/if}
  {/if}
  <Button variant="outline" class="min-h-11 self-start" onclick={() => go(`/objects/${objectId}/reminders/new?kind=reading`)}>{$t('weight.reminder')}</Button>
</section>
