<script lang="ts">
  import { counter, perCounter } from './format';
  import { formatPerUnit, fuelUnitLabel } from './energy';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { CounterUnit, EnergyOut } from './types';

  /** `energy` is loaded by the parent alongside the timeline's own energy rate (not only while
   *  the Info tab is open -- see `loadEnergy` in ObjectDetail.svelte), the same "load once, hand
   *  down" pattern `TripTotals`'s `summary` uses. `null` covers both "not loaded yet" and a
   *  failed request, treated the same as "nothing to show" -- see `TripTotals.svelte`'s own
   *  doc comment for why. `unit` is the object's counter unit (km/mi/h); a `null` one (an
   *  object with a fuel unit but no counter, or historical rows from before one was cleared)
   *  hides the distance-based rows below the same way each row itself does -- see `hasAny`. */
  let { energy, unit }: { energy: EnergyOut | null; unit: CounterUnit } = $props();

  // Mirrors each row's own guard below exactly, rather than only checking the underlying
  // figures: `distance_per_charge`/`distance_per_unit_milli` can be non-null on data logged
  // before the object's counter unit was cleared, and a heading with every row hidden beneath
  // it is worse than no heading at all.
  const hasAny = $derived(
    energy !== null
    && ((energy.distance_per_charge !== null && !!unit)
      || (energy.distance_per_unit_milli !== null && !!energy.unit && !!unit)
      || (energy.cost_per_counter_milli !== null && !!unit)
      || !!energy.battery),
  );
</script>

{#if hasAny && energy}
  <h3>{$t('energy.title')}</h3>
  {#if energy.distance_per_charge !== null && unit}
    <p class="muted">{$t('energy.distance-per-charge')}: <b>{counter(energy.distance_per_charge, unit, $locale)}</b></p>
  {/if}
  {#if energy.distance_per_unit_milli !== null && energy.unit && unit}
    <p class="muted">{$t('energy.distance-per-unit')}: <b>{formatPerUnit(energy.distance_per_unit_milli, fuelUnitLabel(energy.unit), unit, $locale)}</b></p>
  {/if}
  {#if energy.cost_per_counter_milli !== null && unit}
    <!-- Per 100 units, not per single unit: `insights.consumption` already reads "/100 km",
         and a single-unit rate here would be both an odd fraction of a cent (a plain "€0.01"
         claims far less precision than the mean it actually is) and ~40 % off once rounded to
         cents at all -- `cost_per_counter_milli` (600 -> €0.006/km) rounds to €0.01, while the
         same rate over 100 units (60 000 milli -> €0.60) rounds true. -->
    <p class="muted">{$t('energy.cost-per-100', { unit })}: <b>{perCounter(energy.cost_per_counter_milli * 100, $currency, $locale)}</b></p>
  {/if}
  {#if energy.battery}
    <p class="muted" data-testid="energy-battery">
      {$t('energy.charge-due')}: <b>≈ {energy.battery.remaining_pct} % {#if energy.battery.range_left !== null && unit}· ≈ {counter(energy.battery.range_left, unit, $locale)}{/if}</b>{#if energy.battery.warn} · {$t('energy.charge-soon')}{/if}
    </p>
  {/if}
{/if}
