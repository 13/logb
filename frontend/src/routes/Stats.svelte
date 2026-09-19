<script lang="ts">
  import TopBar from '../lib/TopBar.svelte';
  import BarList, { type Bar } from '../lib/BarList.svelte';
  import { api } from '../lib/api';
  import { go } from '../lib/router';
  import { money } from '../lib/format';
  import { currency } from '../stores/session';
  import { persisted } from '../stores/persisted';
  import { locale, t } from '../i18n';
  import { PURCHASE_PRICE, flattenTree, periodLabel, sharePct, statsPath } from '../lib/stats';
  import type { Amount, EnergyUsage, FuelUsage, Stats } from '../lib/types';
  import { customTypes, typeLabel, typesLoaded } from '../lib/type-registry';

  /** Per device and not synced: whether to count purchase prices is a way of looking, not data. */
  const includePurchases = persisted('logb.stats.purchases', false);

  /** Four digits from `?year=`, or all years. Anything else in the address is ignored rather than
   *  sent to the API to be refused. */
  const yearFromUrl = new URLSearchParams(location.search).get('year');
  let year = $state<string | null>(yearFromUrl && /^\d{4}$/.test(yearFromUrl) ? yearFromUrl : null);

  // The year stays in the address, as Search keeps its query: a reload, a shared link or the back
  // button from an object returns to the same year. Replaced only when it changes, so opening
  // `/stats` does not rewrite its own address.
  $effect(() => {
    const url = new URL(location.href);
    if (year) url.searchParams.set('year', year); else url.searchParams.delete('year');
    const next = url.pathname + url.search;
    if (next !== location.pathname + location.search) history.replaceState(null, '', next);
  });
  let data = $state<Stats | null>(null);
  let energy = $state<EnergyUsage | null>(null);
  let fuel = $state<FuelUsage | null>(null);
  /** The last year list seen. Kept apart from `data`, which is cleared on every fetch, so the
   *  picker does not empty and reset itself while the next selection loads. */
  let years = $state<string[]>([]);
  let error = $state('');
  let expanded = $state<Set<number>>(new Set());

  $effect(() => {
    const path = statsPath(year, $includePurchases);
    // Cleared first: a stale total under a new selection would be a wrong number on screen.
    data = null;
    energy = null;
    fuel = null;
    error = '';
    let current = true;
    Promise.all([api<Stats>('GET', path), api<EnergyUsage>('GET', '/stats/energy'), api<FuelUsage>('GET', '/stats/fuel')])
      .then(([d, e, f]) => { if (current) { data = d; energy = e; fuel = f; years = d.years; } })
      .catch((e) => { if (current) error = (e as Error).message; });
    return () => { current = false; };
  });

  /** A selected year that has no spend any more (the toggle was turned off, say) stays offered,
   *  so the picker never shows a blank. */
  const yearOptions = $derived(year && !years.includes(year) ? [year, ...years] : years);

  function toggle(id: number) {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id); else next.add(id);
    expanded = next;
  }

  const fmt = (cents: number) => money(cents, $currency, $locale);
  const fmtKwh = (milli: number) => `${new Intl.NumberFormat($locale, { maximumFractionDigits: 1 }).format(milli / 1000)} kWh`;
  const fmtLiters = (milli: number) => `${new Intl.NumberFormat($locale, { maximumFractionDigits: 1 }).format(milli / 1000)} L`;
  const fmtGallons = (milli: number) => `${new Intl.NumberFormat($locale, { maximumFractionDigits: 1 }).format(milli / 1000)} gal`;
  const share = (cents: number) => `${fmt(cents)} · ${sharePct(cents, data?.total_cents ?? 0)}%`;
  const bars = (list: Amount[], label: (bucket: string) => string): Bar[] =>
    list.map((a) => ({ key: a.bucket, label: label(a.bucket), value: a.cost_cents, display: share(a.cost_cents) }));

  const objectBars = $derived<Bar[]>(
    data
      ? flattenTree(data.by_object, expanded).map((r) => ({
          key: r.node.id,
          label: r.node.name,
          value: r.node.cost_cents,
          display: share(r.node.cost_cents),
          depth: r.depth,
          note: r.node.archived ? $t('stats.archived') : undefined,
          onLabel: () => go(`/objects/${r.node.id}`),
          expanded: r.hasChildren ? r.expanded : undefined,
          onToggle: () => toggle(r.node.id),
          toggleLabel: $t(r.expanded ? 'stats.collapse' : 'stats.expand', { name: r.node.name }),
        }))
      : [],
  );
</script>

<main>
  <TopBar title={$t('stats.title')} icon="chart" />

  <div class="field">
    <label for="stats-year">{$t('stats.year')}</label>
    <select id="stats-year" value={year ?? ''} onchange={(e) => (year = (e.currentTarget as HTMLSelectElement).value || null)}>
      <option value="">{$t('stats.all-years')}</option>
      {#each yearOptions as y (y)}<option value={y}>{y}</option>{/each}
    </select>
  </div>
  <label class="row toggle"><input type="checkbox" bind:checked={$includePurchases} /> {$t('stats.purchases')}</label>

  {#if energy}
    <section data-testid="stats-energy">
      <h2>{$t('stats.energy-title')}</h2>
      <p class="total">{$t('stats.energy-current')}: <b class="tnum">{fmtKwh(energy.current_kwh_milli)}</b></p>
      <p class="muted">{$t('stats.energy-previous')}: {fmtKwh(energy.previous_kwh_milli)}</p>
      <BarList items={energy.months.map((m) => ({ key: m.month, label: periodLabel(m.month, $locale), value: m.kwh_milli, display: fmtKwh(m.kwh_milli) }))} />
    </section>
  {/if}
  {#if fuel}
    <section data-testid="stats-fuel">
      <h2>{$t('stats.fuel-title')}</h2>
      <p class="total">{$t('stats.energy-current')}: <b class="tnum">{fmtLiters(fuel.current_liters_milli)} · {fmtGallons(fuel.current_gallons_milli)}</b></p>
      <p class="muted">{$t('stats.energy-previous')}: {fmtLiters(fuel.previous_liters_milli)} · {fmtGallons(fuel.previous_gallons_milli)}</p>
      <h3>{$t('stats.fuel-liters')}</h3>
      <BarList items={fuel.months.map((m) => ({ key: m.month, label: periodLabel(m.month, $locale), value: m.liters_milli, display: fmtLiters(m.liters_milli) }))} />
      <h3>{$t('stats.fuel-gallons')}</h3>
      <BarList items={fuel.months.map((m) => ({ key: m.month, label: periodLabel(m.month, $locale), value: m.gallons_milli, display: fmtGallons(m.gallons_milli) }))} />
    </section>
  {/if}

  {#if error}
    <p class="error">{error}</p>
  {:else if !data}
    <p class="muted">{$t('nav.loading')}</p>
  {:else if data.total_cents === 0}
    <div class="empty"><p>{$t('stats.none')}</p></div>
  {:else}
    <p class="total" data-testid="stats-total">{$t('stats.total')}: <b class="tnum">{fmt(data.total_cents)}</b></p>

    <section data-testid="stats-over-time">
      <h2>{$t('stats.over-time')}</h2>
      <!-- Months show the amount alone: a share of the year on every one of twelve bars is noise. -->
      <BarList items={data.over_time.map((a) => ({ key: a.bucket, label: periodLabel(a.bucket, $locale), value: a.cost_cents, display: fmt(a.cost_cents) }))} />
    </section>

    <section data-testid="stats-by-object">
      <h2>{$t('stats.by-object')}</h2>
      <BarList items={objectBars} />
    </section>

    <section data-testid="stats-by-type">
      <h2>{$t('stats.by-type')}</h2>
      <BarList items={bars(data.by_type, (b) => typeLabel(b, $customTypes, $t, $typesLoaded))} />
    </section>

    <section data-testid="stats-by-category">
      <h2>{$t('stats.by-category')}</h2>
      <BarList items={bars(data.by_category, (b) => (b === PURCHASE_PRICE ? $t('stats.purchase-price') : $t(`cat.${b}`)))} />
    </section>
  {/if}
</main>

<style>
  /* .field, .row, .muted, .error, .empty, h2 and .tnum are global -- the rules below are specific
     to this screen. */
  .total { font-size: var(--text-lg); margin: var(--space-3) 0; }
  /* BarList's default 90px label column fits "Fuel" but clips an object name or "Maintenance". */
  section :global(.label) { width: 140px; }
</style>
