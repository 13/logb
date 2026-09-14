<script lang="ts">
  import { api } from './api';
  import { counter, money, perCounter, quantity } from './format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { CounterUnit, Insights } from './types';

  let { objectId, unit }: { objectId: number; unit: CounterUnit } = $props();

  let data = $state<Insights | null>(null);
  let error = $state('');

  $effect(() => {
    objectId;
    // Reset before the fetch, not just on success: without this, switching to another object
    // shows the previous one's cost breakdown under the new object's name for however long the
    // request takes -- and indefinitely if it fails, since neither `data` nor `error` was ever
    // touched for the new id.
    data = null;
    error = '';
    api<Insights>('GET', `/objects/${objectId}/insights`)
      .then((d) => (data = d))
      .catch((e) => (error = (e as Error).message));
  });

  /** Bar width as a percentage of the largest bucket, so the widest bar always fills the row.
   *  No negative-width guard: the API rejects cost_cents < 0 at the boundary
   *  (see `ActivityInput::validate` in src/api/activities.rs), so `value` is never negative here. */
  function pct(value: number, all: { cost_cents: number }[]): number {
    const max = Math.max(...all.map((b) => b.cost_cents), 1);
    return Math.round((value / max) * 100);
  }

  /** `2026-09` as the reader's short month and year, e.g. "Sep 26". */
  function monthLabel(month: string): string {
    const [y, m] = month.split('-').map(Number);
    return new Intl.DateTimeFormat($locale, { month: 'short', year: '2-digit' }).format(new Date(Date.UTC(y, m - 1, 15)));
  }
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if data && data.counter_per_day_milli !== null && unit}
  <!-- A month is the unit people think in for mileage; 30.44 days is the average one. Rounded
       to a whole unit, since the rate is an average and more digits would claim precision it
       does not have. -->
  <p class="muted">{$t('insights.usage')}: <b>{$t('insights.per-month', { amount: counter(Math.round(data.counter_per_day_milli * 30.44 / 1000), unit, $locale) })}</b></p>
{/if}
{#if data && data.usage_by_month.length > 0 && unit}
  {@const measured = data.usage_by_month.map((m) => ({ cost_cents: m.amount ?? 0 }))}
  <h3>{$t('insights.usage-by-month')}</h3>
  {#each data.usage_by_month as m, i (m.month)}
    <div class="bar-row">
      <span class="label">{monthLabel(m.month)}</span>
      <span class="track"><span class="fill" style={`width:${pct(measured[i].cost_cents, measured)}%`}></span></span>
      <!-- A month the readings cannot measure says so, rather than drawing a zero it does not know. -->
      <span class="value tnum">{m.amount === null ? '—' : counter(m.amount, unit, $locale)}</span>
    </div>
  {/each}
{/if}
{#if data}
  {#if data.by_year.length === 0}
    <p class="muted">{$t('insights.none')}</p>
  {:else}
    <h3>{$t('insights.by-year')}</h3>
    {#each data.by_year as b (b.bucket)}
      <div class="bar-row">
        <span class="label">{b.bucket}</span>
        <span class="track"><span class="fill" style={`width:${pct(b.cost_cents, data.by_year)}%`}></span></span>
        <span class="value">{money(b.cost_cents, $currency, $locale)}</span>
      </div>
    {/each}

    <h3>{$t('insights.by-category')}</h3>
    {#each data.by_category as b (b.bucket)}
      <div class="bar-row">
        <span class="label">{$t(`cat.${b.bucket}`)}</span>
        <span class="track"><span class="fill" style={`width:${pct(b.cost_cents, data.by_category)}%`}></span></span>
        <span class="value">{money(b.cost_cents, $currency, $locale)}</span>
      </div>
    {/each}

    {#if data.cost_per_counter_milli !== null && unit}
      <p class="muted">{$t('insights.per-counter', { unit })}: <b>{perCounter(data.cost_per_counter_milli, $currency, $locale)}</b></p>
    {/if}
    {#if data.fuel}
      <p class="muted">{$t('insights.fuel-total')}: <b>{quantity(data.fuel.quantity_milli, data.fuel.unit, $locale)}</b></p>
      {#if data.fuel.per_100_milli !== null && unit}
        <p class="muted">{$t('insights.consumption')}: <b>{quantity(data.fuel.per_100_milli, data.fuel.unit, $locale)}/100 {unit}</b></p>
        <!-- `cost_per_counter_milli` is null under exactly the same condition as `per_100_milli`
             (both need >= 2 fills spanning a positive counter distance -- see
             `fuel_cost_per_counter_milli` / `consumption_per_100_milli` in
             src/domain/insights.rs), so this guard covers both. Same scale as the overall
             per-counter figure above (milli-cents per unit), hence the same `perCounter`
             formatter. -->
        <p class="muted">{$t('insights.fuel-per-counter', { unit })}: <b>{perCounter(data.fuel.cost_per_counter_milli, $currency, $locale)}</b></p>
      {/if}
    {/if}
  {/if}
{/if}

<style>
  h3 { margin: var(--space-4) 0 var(--space-2); font-size: var(--text-base); }
  .bar-row { display: flex; align-items: center; gap: var(--space-2); margin-bottom: var(--space-2); }
  .label { flex: none; width: 90px; font-size: var(--text-sm); }
  /* Half the track's height, spelled as a literal, is a pill -- and a pill is a token. */
  .track { flex: 1; height: 10px; background: var(--surface-2); border-radius: var(--radius-full); overflow: hidden; }
  .fill { display: block; height: 100%; background: var(--accent); }
  .value { flex: none; font-size: var(--text-sm); }
</style>
