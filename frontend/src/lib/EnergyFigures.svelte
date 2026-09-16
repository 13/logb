<script lang="ts">
  import { counter, perCounter } from './format';
  import { formatPerUnit } from './energy';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { CounterUnit, EnergyOut } from './types';

  /** `energy` is loaded by the parent alongside the timeline's own energy rate (not only while
   *  the Info tab is open -- see `loadEnergy` in ObjectDetail.svelte), the same "load once, hand
   *  down" pattern `TripTotals`'s `summary` uses. `null` covers both "not loaded yet" and a
   *  failed request, treated the same as "nothing to show" -- see `TripTotals.svelte`'s own
   *  doc comment for why. `unit` is the object's counter unit (km/mi/h), for the distance
   *  figures; a `null` one (an object with a fuel unit but no counter) simply shows no
   *  distance-based row, `hasAny` below never becoming true from those alone. */
  let { energy, unit }: { energy: EnergyOut | null; unit: CounterUnit } = $props();

  const hasAny = $derived(
    energy !== null
    && (energy.distance_per_charge !== null || energy.distance_per_unit_milli !== null
      || energy.cost_per_counter_milli !== null || energy.battery !== null),
  );
</script>

{#if hasAny && energy}
  <h3>{$t('energy.title')}</h3>
  {#if energy.distance_per_charge !== null && unit}
    <p class="muted">{$t('energy.distance-per-charge')}: <b>{counter(energy.distance_per_charge, unit, $locale)}</b></p>
  {/if}
  {#if energy.distance_per_unit_milli !== null && energy.unit && unit}
    <p class="muted">{$t('energy.distance-per-unit')}: <b>{formatPerUnit(energy.distance_per_unit_milli, energy.unit, unit, $locale)}</b></p>
  {/if}
  {#if energy.cost_per_counter_milli !== null}
    <p class="muted">{$t('energy.cost-per-distance')}: <b>{perCounter(energy.cost_per_counter_milli, $currency, $locale)}</b></p>
  {/if}
  {#if energy.battery}
    <p class="muted" data-testid="energy-battery">
      ≈ {energy.battery.remaining_pct} %
      {#if energy.battery.range_left !== null && unit} · ≈ {counter(energy.battery.range_left, unit, $locale)}{/if}
      {#if energy.battery.warn} · {$t('energy.charge-soon')}{/if}
    </p>
  {/if}
{/if}
