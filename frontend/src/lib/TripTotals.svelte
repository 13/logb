<script lang="ts">
  import { counter } from './format';
  import { formatPer10Pct, formatSpeed } from './trip';
  import { locale, t } from '../i18n';
  import type { TripSummary } from './types';

  /** `summary` is loaded by the parent (like `LastDone`'s `items`), fetched once when the Info
   *  tab opens -- see `loadTripSummary` in ObjectDetail.svelte. `null` covers both "not loaded
   *  yet" and a failed request, and is treated the same as "no trips": a table of dashes over a
   *  request that just failed would read as data, not as the harmless miss it is. `unit` is
   *  always `'km'`/`'mi'` here -- the parent only renders this component on an object that
   *  offers trips at all, which needs exactly one of those two counter units. */
  let { summary, unit }: { summary: TripSummary | null; unit: 'km' | 'mi' } = $props();

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
  <h3>{$t('trips.title')}</h3>
  <!-- `tabindex="0"` and `aria-label` (the heading text, since the scrolled box has no visible
       heading of its own) let a keyboard/AT user reach and identify the scroller even when the
       table inside it does not overflow at their width. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="table-wrap" data-testid="trip-totals" tabindex="0" aria-label={$t('trips.title')}>
    <table class="tnum">
      <thead>
        <tr>
          <th></th>
          {#each periods as p (p.key)}<th scope="col">{p.label}</th>{/each}
        </tr>
      </thead>
      <tbody>
        <tr>
          <th scope="row">{$t('trips.count')}</th>
          {#each periods as p (p.key)}<td>{p.totals.trips}</td>{/each}
        </tr>
        <tr>
          <th scope="row">{$t('trips.distance')}</th>
          {#each periods as p (p.key)}<td>{counter(p.totals.distance, unit, $locale)}</td>{/each}
        </tr>
        <tr>
          <th scope="row">{$t('trips.avg')}</th>
          {#each periods as p (p.key)}<td>{p.totals.avg_distance === null ? '–' : counter(p.totals.avg_distance, unit, $locale)}</td>{/each}
        </tr>
        {#if showSpeed}
          <tr>
            <th scope="row">{$t('trips.speed')}</th>
            {#each periods as p (p.key)}<td>{p.totals.speed_x10 === null ? '–' : formatSpeed(p.totals.speed_x10, unit, $locale)}</td>{/each}
          </tr>
        {/if}
        {#if showBattery}
          <tr>
            <!-- The cells themselves are a plain distance (`formatPer10Pct`) -- this label is
                 the only place "per 10 %" is said at all. -->
            <th scope="row">{$t('trips.per-battery')}</th>
            {#each periods as p (p.key)}<td>{p.totals.distance_per_10pct === null ? '–' : formatPer10Pct(p.totals.distance_per_10pct, unit, $locale)}</td>{/each}
          </tr>
        {/if}
      </tbody>
    </table>
  </div>
{/if}

<style>
  /* Four columns (row label + three periods) can outrun 390px with longer German labels --
     scrolled inside its own box rather than shrinking the page, like `.hint`-adjacent tables
     elsewhere never need to (there are none); this is the first. Kept as a belt-and-braces
     fallback even with the row labels wrapping and the narrower padding below, which between
     them keep all three period columns on screen at 390px without it. */
  .table-wrap { overflow-x: auto; }
  table { border-collapse: collapse; width: 100%; }
  th, td { padding: var(--space-1); text-align: right; white-space: nowrap; }
  thead th { font-weight: 600; }
  /* Row labels wrap rather than force the table wider than the screen: "Ø Geschwindigkeit" is
     longer than any single period's own values, and a wrapped two-line label costs far less
     horizontal room than a `nowrap` one that pushes the value columns off a 390px screen. */
  tbody th { text-align: left; font-weight: 400; color: var(--muted); white-space: normal; }
</style>
