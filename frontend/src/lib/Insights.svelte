<script lang="ts">
  import BarList from './BarList.svelte';
  import Chart from './Chart.svelte';
  import { counter, money, moneyWhole, perCounter, quantity } from './format';
  import { energyLabelKey, fuelUnitLabel } from './energy';
  import { fillLabel, fillTick, monthLabel, monthTick, sinceLabel } from './insights';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { CounterUnit, FuelUnit, Insights } from './types';

  /** Loaded by ObjectDetail, which needs the same answer for the summary's consumption figure
   *  and the spend chart: one request, not two. `null` until it arrives. `contents` is the
   *  "Include contents" switch; ObjectDetail owns it because it decides which request is made.
   *  `hasContents` offers the switch only where there is something to include. */
  let { data, error = '', unit, hasContents = false, contents = false, oncontents }: {
    data: Insights | null; error?: string; unit: CounterUnit; hasContents?: boolean; contents?: boolean;
    oncontents?: (on: boolean) => void;
  } = $props();

  const fmt = (cents: number) => money(cents, $currency, $locale);
  const spent = $derived(data ? data.by_year.some((b) => b.cost_cents > 0) : false);
</script>

{#if hasContents}
  <label class="row toggle">
    <input type="checkbox" checked={contents} onchange={(e) => oncontents?.(e.currentTarget.checked)} />
    {$t('insights.contents')}
  </label>
{/if}
{#if error}<p class="error" role="alert">{error}</p>{/if}
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

    <h3 class="m-0 mt-4 mb-2 text-sm font-semibold text-foreground">{$t('insights.by-year')}</h3>
    <BarList items={data.by_year.map((b) => ({ key: b.bucket, label: b.bucket, value: b.cost_cents, display: fmt(b.cost_cents) }))} />

    <h3 class="m-0 mt-4 mb-2 text-sm font-semibold text-foreground">{$t('insights.by-category')}</h3>
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
      <!-- Petrol only. `cost_per_counter_milli` is null under exactly the same condition as
           `per_100_milli` (both need >= 2 fills spanning a positive counter distance -- see
           `fuel_cost_per_counter_milli` / `consumption_per_100_milli` in
           src/domain/insights.rs), so this guard covers both.

           A kWh object gets no cost row here at all: the Energy section states the same quantity
           for the same object, under the same "at least two charges" condition, but measures it
           differently -- full-charge-to-full-charge windows with the object's price filling in a
           missing cost, against this one's whole-span average over every fill that counts an
           unrecorded cost as zero. Two rows, one label, two numbers (€0.60 here, €0.00 there for
           a priced object whose charges carry no cost) reads as a bug, so the authoritative one
           is the only one shown. -->
      {#if !charged}
        <p class="muted">
          {$t('insights.fuel-per-counter', { unit })}:
          <b>{perCounter(fuel.cost_per_counter_milli, $currency, $locale)}</b>
        </p>
      {/if}
    {/if}
  {/if}
  {#if data.counter_per_day_milli !== null && unit}
    <!-- A month is the unit people think in for mileage; 30.44 days is the average one. Rounded
         to a whole unit, since the rate is an average and more digits would claim precision it
         does not have. -->
    <p class="muted">{$t('insights.usage')}: <b>{$t('insights.per-month', { amount: counter(Math.round(data.counter_per_day_milli * 30.44 / 1000), unit, $locale) })}</b></p>
  {/if}
  {#if data.usage_by_month.some((m) => (m.amount ?? 0) > 0) && unit}
    <!-- A month the readings cannot measure is left out with the empty ones, rather than drawn
         as a zero it does not know. -->
    <section data-testid="insights-usage">
      <h3 class="m-0 mt-4 mb-2 text-sm font-semibold text-foreground">{$t('insights.usage-by-month')}</h3>
      <Chart label={$t('insights.usage-by-month')} items={data.usage_by_month.map((m) => ({
        key: m.month, label: monthLabel(m.month, $locale), tick: monthTick(m.month, $locale), value: m.amount ?? 0,
        display: m.amount === null ? '—' : counter(m.amount, unit, $locale),
      }))} />
    </section>
  {/if}
  {#if data.trip_distance_by_month.some((m) => m.distance > 0) && unit}
    <section data-testid="insights-trip-distance">
      <h3 class="m-0 mt-4 mb-2 text-sm font-semibold text-foreground">{$t('trips.by-month')}</h3>
      <Chart label={$t('trips.by-month')} items={data.trip_distance_by_month.map((m) => ({
        key: m.month, label: monthLabel(m.month, $locale), tick: monthTick(m.month, $locale), value: m.distance, display: counter(m.distance, unit, $locale),
      }))} />
    </section>
  {/if}
  {#if data.fuel && data.fuel.fills.length > 0 && unit}
    {@const fuel = data.fuel}
    <section data-testid="insights-by-fill">
      <h3 class="m-0 mt-4 mb-2 text-sm font-semibold text-foreground">{$t('insights.by-fill')}</h3>
      <p class="m-0 mb-2 text-sm text-muted-foreground">{$t('insights.by-fill-hint')}</p>
      <Chart label={$t('insights.by-fill')} items={fuel.fills.map((f, i) => ({
        key: `${f.date}-${i}`, label: fillLabel(f.date, $locale), tick: fillTick(f.date, $locale), value: f.per_100_milli,
        display: `${quantity(f.per_100_milli, fuelUnitLabel(fuel.unit), $locale)}/100 ${unit}`,
      }))} />
    </section>
  {/if}
{/if}
