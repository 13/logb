<script lang="ts">
  import { counter } from './format';
  import { energyCost } from './energy';
  import { formatPer10Pct, formatSpeed } from './trip';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { TripSummary } from './types';

  /** `summary` is loaded by the parent (like `LastDone`'s `items`), fetched once when the Info
   *  tab opens -- see `loadTripSummary` in ObjectDetail.svelte. `null` covers both "not loaded
   *  yet" and a failed request, and is treated the same as "no trips": a table of dashes over a
   *  request that just failed would read as data, not as the harmless miss it is. `unit` is
   *  always `'km'`/`'mi'` here -- the parent only renders this component on an object that
   *  offers trips at all, which needs exactly one of those two counter units. `energyRate` is
   *  the object's `cost_per_counter_milli` (see EnergyOut), loaded alongside the Info tab's
   *  Energy section; `null` hides the "Energy cost" row entirely rather than showing dashes. */
  let { summary, unit, energyRate = null }: { summary: TripSummary | null; unit: 'km' | 'mi'; energyRate?: number | null } = $props();

  const periods = $derived(summary
    ? ([
        { key: 'month', label: $t('trips.month'), totals: summary.month },
        { key: 'year', label: $t('trips.year'), totals: summary.year },
        { key: 'all', label: $t('trips.all'), totals: summary.all },
      ] as const)
    : []);

  // Speed and battery rows only exist at all once some trip in some period carries the value --
  // otherwise every cell in the row would read "–", which is worse than not showing the row.
  const showSpeed = $derived(periods.some((p) => p.totals.speed_x10 !== null));
  const showBattery = $derived(periods.some((p) => p.totals.distance_per_10pct !== null));
</script>

{#if summary && summary.all.trips > 0}
  <section><h2 class="m-0 mb-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase">{$t('trips.title')}</h2>
  <!-- `tabindex="0"` and `aria-label` (the heading text, since the scrolled box has no visible
       heading of its own) let a keyboard/AT user reach and identify the scroller even when the
       table inside it does not overflow at their width. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="overflow-x-auto focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring" data-testid="trip-totals" tabindex="0" aria-label={$t('trips.title')}>
    <table class="w-full border-collapse text-sm tabular-nums">
      <thead>
        <tr>
          <th class="p-1"></th>
          {#each periods as p (p.key)}<th scope="col" class="p-1 text-right font-semibold whitespace-nowrap">{p.label}</th>{/each}
        </tr>
      </thead>
      <!-- Row labels wrap rather than force the table wider than the screen: "Ø Geschwindigkeit" is
           longer than any single period's own values, and a wrapped two-line label costs far less
           horizontal room than a `nowrap` one that pushes the value columns off a 390px screen.
           The scroller around the table is the fallback for anything still wider. -->
      <tbody>
        <tr>
          <th scope="row" class="p-1 text-left font-normal text-muted-foreground">{$t('trips.count')}</th>
          {#each periods as p (p.key)}<td class="p-1 text-right whitespace-nowrap">{p.totals.trips}</td>{/each}
        </tr>
        <tr>
          <th scope="row" class="p-1 text-left font-normal text-muted-foreground">{$t('trips.distance')}</th>
          {#each periods as p (p.key)}<td class="p-1 text-right whitespace-nowrap">{counter(p.totals.distance, unit, $locale)}</td>{/each}
        </tr>
        <tr>
          <th scope="row" class="p-1 text-left font-normal text-muted-foreground">{$t('trips.avg')}</th>
          {#each periods as p (p.key)}<td class="p-1 text-right whitespace-nowrap">{p.totals.avg_distance === null ? '–' : counter(p.totals.avg_distance, unit, $locale)}</td>{/each}
        </tr>
        {#if showSpeed}
          <tr>
            <th scope="row" class="p-1 text-left font-normal text-muted-foreground">{$t('trips.speed')}</th>
            {#each periods as p (p.key)}<td class="p-1 text-right whitespace-nowrap">{p.totals.speed_x10 === null ? '–' : formatSpeed(p.totals.speed_x10, unit, $locale)}</td>{/each}
          </tr>
        {/if}
        {#if showBattery}
          <tr>
            <!-- The cells themselves are a plain distance (`formatPer10Pct`) -- this label is
                 the only place "per 10 %" is said at all. -->
            <th scope="row" class="p-1 text-left font-normal text-muted-foreground">{$t('trips.per-battery')}</th>
            {#each periods as p (p.key)}<td class="p-1 text-right whitespace-nowrap">{p.totals.distance_per_10pct === null ? '–' : formatPer10Pct(p.totals.distance_per_10pct, unit, $locale)}</td>{/each}
          </tr>
        {/if}
        {#if energyRate !== null}
          <tr>
            <th scope="row" class="p-1 text-left font-normal text-muted-foreground">{$t('trips.energy-cost')}</th>
            {#each periods as p (p.key)}<td class="p-1 text-right whitespace-nowrap">{energyCost(p.totals.distance, energyRate, $currency, $locale)}</td>{/each}
          </tr>
        {/if}
      </tbody>
    </table>
  </div>
  </section>
{/if}

