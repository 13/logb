<script lang="ts">
  import { api } from './api';
  import BarList from './BarList.svelte';
  import { counter, money, moneyWhole, perCounter, quantity } from './format';
  import { energyLabelKey, fuelUnitLabel } from './energy';
  import { fillLabel, insightsPath, monthLabel, sinceLabel } from './insights';
  import { persisted } from '../stores/persisted';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { CounterUnit, FuelUnit, Insights } from './types';

  let { objectId, unit, hasContents = false }: { objectId: number; unit: CounterUnit; hasContents?: boolean } = $props();

  /** Per device and not synced, like the Statistics screen's purchase switch: a way of looking, not data. */
  const includeContents = persisted('logb.insights.contents', false);

  // An object without children always asks for its own figures, whatever the switch last said
  // on a house. Derived (not read directly in the effect below) so a `hasContents` flip that
  // doesn't change the resulting path -- e.g. `children` finishing its own load after Insights
  // has already mounted -- doesn't re-trigger the fetch.
  const path = $derived(insightsPath(objectId, hasContents && $includeContents));

  let data = $state<Insights | null>(null);
  let error = $state('');

  $effect(() => {
    const p = path;
    // Reset before the fetch, not just on success: without this, switching to another object
    // shows the previous one's cost breakdown under the new object's name for however long the
    // request takes -- and indefinitely if it fails, since neither `data` nor `error` was ever
    // touched for the new id.
    data = null;
    error = '';
    let current = true;
    api<Insights>('GET', p)
      .then((d) => { if (current) data = d; })
      .catch((e) => { if (current) error = (e as Error).message; });
    return () => { current = false; };
  });

  const fmt = (cents: number) => money(cents, $currency, $locale);
  const spent = $derived(data ? data.by_year.some((b) => b.cost_cents > 0) : false);
</script>

{#if hasContents}
  <label class="row toggle">
    <input type="checkbox" bind:checked={$includeContents} />
    {$t('insights.contents')}
  </label>
{/if}
{#if error}<p class="error">{error}</p>{/if}
{#if data}
  {#if data.ownership.total_cents > 0}
    {@const o = data.ownership}
    <p data-testid="insights-ownership">
      {$t('insights.ownership')}: <b class="tnum">{fmt(o.total_cents)}</b>
      <span class="muted">· {o.per_year_cents !== null
        ? $t('insights.per-year-since', { amount: moneyWhole(o.per_year_cents, $currency, $locale), since: sinceLabel(o.since, $locale) })
        : $t('insights.since', { since: sinceLabel(o.since, $locale) })}</span>
    </p>
  {/if}
  {#if !spent}
    <p class="muted">{$t('insights.none')}</p>
  {:else}
    <h3>{$t('insights.spend-by-month')}</h3>
    <BarList items={data.by_month.map((b) => ({ key: b.bucket, label: monthLabel(b.bucket, $locale), value: b.cost_cents, display: fmt(b.cost_cents) }))} />

    <h3>{$t('insights.by-year')}</h3>
    <BarList items={data.by_year.map((b) => ({ key: b.bucket, label: b.bucket, value: b.cost_cents, display: fmt(b.cost_cents) }))} />

    <h3>{$t('insights.by-category')}</h3>
    <BarList items={data.by_category.map((b) => ({ key: b.bucket, label: $t(`cat.${b.bucket}`), value: b.cost_cents, display: fmt(b.cost_cents) }))} />

    {#if data.cost_per_counter_milli !== null && unit}
      <p class="muted">{$t('insights.per-counter', { unit })}: <b>{perCounter(data.cost_per_counter_milli, $currency, $locale)}</b></p>
    {/if}
  {/if}
  {#if data.fuel}
    {@const fuel = data.fuel}
    <!-- A kWh object reads "Charged"/"Energy cost per {unit}" here instead of the petrol
         wording -- `energyLabelKey` also drives the "+ Log charge" button and the Energy
         section's own log button, so a kWh object never mixes the two vocabularies. -->
    {@const charged = energyLabelKey(fuel.unit as FuelUnit) === 'energy.charged'}
    <p class="muted">{$t(charged ? 'energy.charged-total' : 'insights.fuel-total')}: <b>{quantity(fuel.quantity_milli, fuelUnitLabel(fuel.unit), $locale)}</b></p>
    {#if fuel.per_100_milli !== null && unit}
      <p class="muted">{$t('insights.consumption')}: <b>{quantity(fuel.per_100_milli, fuelUnitLabel(fuel.unit), $locale)}/100 {unit}</b></p>
      <!-- `cost_per_counter_milli` is null under exactly the same condition as `per_100_milli`
           (both need >= 2 fills spanning a positive counter distance -- see
           `fuel_cost_per_counter_milli` / `consumption_per_100_milli` in
           src/domain/insights.rs), so this guard covers both. Same scale as the overall
           per-counter figure above (milli-cents per unit), hence the same `perCounter`
           formatter. -->
      <p class="muted">{$t(charged ? 'energy.charged-cost-per-counter' : 'insights.fuel-per-counter', { unit })}: <b>{perCounter(fuel.cost_per_counter_milli, $currency, $locale)}</b></p>
    {/if}
  {/if}
  {#if data.counter_per_day_milli !== null && unit}
    <!-- A month is the unit people think in for mileage; 30.44 days is the average one. Rounded
         to a whole unit, since the rate is an average and more digits would claim precision it
         does not have. -->
    <p class="muted">{$t('insights.usage')}: <b>{$t('insights.per-month', { amount: counter(Math.round(data.counter_per_day_milli * 30.44 / 1000), unit, $locale) })}</b></p>
  {/if}
  {#if data.usage_by_month.length > 0 && unit}
    <h3>{$t('insights.usage-by-month')}</h3>
    <!-- A month the readings cannot measure says so, rather than drawing a zero it does not know. -->
    <BarList items={data.usage_by_month.map((m) => ({
      key: m.month, label: monthLabel(m.month, $locale), value: m.amount ?? 0,
      display: m.amount === null ? '—' : counter(m.amount, unit, $locale),
    }))} />
  {/if}
  {#if data.trip_distance_by_month.some((m) => m.distance > 0) && unit}
    <section data-testid="insights-trip-distance">
      <h3>{$t('trips.by-month')}</h3>
      <BarList items={data.trip_distance_by_month.map((m) => ({
        key: m.month, label: monthLabel(m.month, $locale), value: m.distance, display: counter(m.distance, unit, $locale),
      }))} />
    </section>
  {/if}
  {#if data.fuel && data.fuel.fills.length > 0 && unit}
    {@const fuel = data.fuel}
    <section data-testid="insights-by-fill">
      <h3>{$t('insights.by-fill')}</h3>
      <p class="muted hint">{$t('insights.by-fill-hint')}</p>
      <BarList items={fuel.fills.map((f, i) => ({
        key: `${f.date}-${i}`, label: fillLabel(f.date, $locale), value: f.per_100_milli,
        display: `${quantity(f.per_100_milli, fuelUnitLabel(fuel.unit), $locale)}/100 ${unit}`,
      }))} />
    </section>
  {/if}
{/if}

<style>
  h3 { margin: var(--space-4) 0 var(--space-2); font-size: var(--text-base); }
  .hint { font-size: var(--text-sm); margin: 0 0 var(--space-2); }
</style>
