<script lang="ts">
  import { onMount } from 'svelte';
  import { numberFormat, dateTimeFormat } from '../lib/intl-cache';
  import { errorMessage } from '../lib/api-error';
  import TopBar from '../lib/TopBar.svelte';
  import BarList, { type Bar } from '../lib/BarList.svelte';
  import Chart from '../lib/Chart.svelte';
  import type { ChartBar } from '../lib/chart';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { api } from '../lib/api';
  import { go } from '../lib/router';
  import { money, todayIso } from '../lib/format';
  import { currency } from '../stores/session';
  import { persisted } from '../stores/persisted';
  import { locale, t } from '../i18n';
  import { PURCHASE_PRICE, flattenTree, monthLabel, periodLabel, sharePct, statsPath, yearSummary, type YearSummary } from '../lib/stats';
  import type { Amount, EnergyUsage, FuelUsage, WaterUsage, Stats } from '../lib/types';
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
  let water = $state<WaterUsage | null>(null);
  /** The last year list seen. Kept apart from `data`, which is cleared on every fetch, so the
   *  picker does not empty and reset itself while the next selection loads. */
  let years = $state<string[]>([]);
  let error = $state('');
  let expanded = $state<Set<number>>(new Set());

  /** `/stats` answers for this visit, per (year, purchases): the summary and the charts often ask
   *  for the same one, and going back to a selection already seen is instant. A failure is not
   *  kept, so the next change asks again. Nothing here outlives the page. */
  const answers = new Map<string, Stats>();
  const asked = new Map<string, Promise<Stats>>();
  function stats(path: string): Promise<Stats> {
    let p = asked.get(path);
    if (!p) {
      p = api<Stats>('GET', path).then(
        (d) => { answers.set(path, d); return d; },
        (e) => { asked.delete(path); throw e; },
      );
      asked.set(path, p);
    }
    return p;
  }

  $effect(() => {
    const path = statsPath(year, $includePurchases);
    error = '';
    // An answer already in hand is shown at once. Otherwise cleared first: a stale total under a
    // new selection would be a wrong number on screen.
    const known = answers.get(path);
    data = known ?? null;
    if (known) { years = known.years; return; }
    let current = true;
    stats(path)
      .then((d) => { if (current) { data = d; years = d.years; } })
      .catch((e) => { if (current) error = errorMessage(e, $t); });
    return () => { current = false; };
  });

  // Energy, fuel and water take neither the year nor the purchases switch, so they are asked for
  // once per visit, not again on every change of either.
  onMount(() => {
    let current = true;
    api<EnergyUsage>('GET', '/stats/energy').then((e) => { if (current) energy = e; }, () => {});
    api<FuelUsage>('GET', '/stats/fuel').then((f) => { if (current) fuel = f; }, () => {});
    api<WaterUsage>('GET', '/stats/water').then((w) => { if (current) water = w; }, () => {});
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
  const fmtKwh = (milli: number) => `${numberFormat($locale, { maximumFractionDigits: 1 }).format(milli / 1000)} kWh`;
  const fmtLiters = (milli: number) => `${numberFormat($locale, { maximumFractionDigits: 1 }).format(milli / 1000)} L`;
  const fmtGallons = (milli: number) => `${numberFormat($locale, { maximumFractionDigits: 1 }).format(milli / 1000)} gal`;
  const fmtWater = (litersMilli: number) => `${numberFormat($locale, { maximumFractionDigits: 2 }).format(litersMilli / 1_000_000)} m³`;
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
  const hasKwh = $derived(energy?.months.some((m) => m.kwh_milli > 0) ?? false);
  const hasLiters = $derived(fuel?.months.some((m) => m.liters_milli > 0) ?? false);
  const hasGallons = $derived(fuel?.months.some((m) => m.gallons_milli > 0) ?? false);
  const hasFuel = $derived(hasLiters || hasGallons || (fuel?.levels.length ?? 0) > 0);
  // Meter deltas are the useful signal for water. Keep the section visible whenever a month
  // has measured volume, even if a backend/client version reports zero entry metadata.
  const hasWater = $derived(water?.months.some((m) => m.liters_milli > 0 || m.entries > 0 || m.cost_cents > 0) ?? false);
  // The summary is one year: the chosen one, or this year under "All years". The charts below
  // keep describing the selection. With a year chosen, the selection's own answer is that year.
  const today = todayIso();
  const focusYear = $derived(year ?? today.slice(0, 4));
  const beforeYear = $derived(String(Number(focusYear) - 1).padStart(4, '0'));
  /** The cards on screen. The previous year's cards stay until the next year's have arrived
   *  (`summaryBusy` meanwhile), so changing the year or the toggle does not blank them. */
  let summary = $state<YearSummary | null>(null);
  let summaryBusy = $state(false);
  let summaryError = $state('');
  $effect(() => {
    const purchases = $includePurchases;
    const fy = focusYear;
    const by = beforeYear;
    let current = true;
    summaryBusy = true;
    // With a year chosen, its answer is the one the charts asked for too (`stats` shares it).
    Promise.all([stats(statsPath(fy, purchases)), stats(statsPath(by, purchases))])
      .then(([f, b]) => { if (current) { summary = yearSummary(f, b, fy, today); summaryError = ''; } })
      .catch((e) => { if (current) { summary = null; summaryError = errorMessage(e, $t); } })
      .finally(() => { if (current) summaryBusy = false; });
    return () => { current = false; };
  });

  const pct = (n: number) => numberFormat($locale, { style: 'percent', signDisplay: 'exceptZero', maximumFractionDigits: 0 }).format(n / 100);
  const signedMoney = (cents: number) => `${cents > 0 ? '+' : ''}${fmt(cents)}`;
  const monthName = (m: number) => dateTimeFormat($locale, { month: 'short' }).format(new Date(Date.UTC(2000, m - 1, 15)));
  /** "Jan–Sep 2025" while the year runs, "2025" once it is over. */
  const period = (s: YearSummary) =>
    s.previous.through === null ? s.previous.year
      : s.previous.through === 1 ? `${monthName(1)} ${s.previous.year}`
      : `${monthName(1)}–${monthName(s.previous.through)} ${s.previous.year}`;
  /** One bar of a series over time. */
  const bar = (bucket: string, value: number, display: string): ChartBar =>
    ({ key: bucket, label: monthLabel(bucket, $locale), tick: periodLabel(bucket, $locale), value, display });

  const panel = 'flex min-w-0 flex-col gap-3 rounded-lg border border-border bg-card p-4 shadow-xs';
  const heading = 'm-0 text-base font-semibold text-foreground';
  const figure = 'flex min-w-0 flex-col gap-1 rounded-lg border border-border bg-card p-4 shadow-xs';
  const figureValue = 'm-0 text-2xl font-semibold tracking-tight text-foreground tabular-nums';
  const line = 'm-0 text-sm text-muted-foreground tabular-nums';
  const strong = 'font-semibold text-foreground';
  const stretched = "min-w-0 cursor-pointer truncate text-left font-semibold text-foreground after:absolute after:inset-0 after:rounded-lg after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-solid focus-visible:after:outline-offset-2 focus-visible:after:outline-ring";
  const open = `${stretched} text-lg`;
  const openLevel = `${stretched} text-base`;
</script>

<main>
  <TopBar title={$t('stats.title')} icon="chart" />

  <div class="mb-4 flex flex-wrap items-end gap-x-6 gap-y-2">
    <Field id="stats-year" label={$t('stats.year')} class="w-full sm:w-48">
      <NativeSelect bind:value={() => year ?? '', (v) => (year = v || null)}>
        <option value="">{$t('stats.all-years')}</option>
        {#each yearOptions as y (y)}<option value={y}>{y}</option>{/each}
      </NativeSelect>
    </Field>
    <CheckField id="stats-purchases" label={$t('stats.purchases')} bind:checked={() => $includePurchases, (on) => includePurchases.set(on)} />
  </div>

  {#if summaryError && !error}
    <!-- The charts below may still have loaded; the summary says it could not, in its place. -->
    <p role="alert" class="m-0 mb-4 text-sm font-medium text-destructive">{$t('stats.summary-failed')} {summaryError}</p>
  {:else if summary && (summary.spent > 0 || summary.previous.cents > 0)}
    {@const s = summary}
    <section data-testid="stats-summary" aria-label={$t('stats.summary', { year: s.year })} aria-busy={summaryBusy} class="mb-6 grid grid-cols-1 gap-3 sm:grid-cols-3">
      <div data-testid="stats-spent" class={figure}>
        <p class={sectionHeadingClass}>{$t('stats.spent-in', { year: s.year })}</p>
        <p class={figureValue}>{fmt(s.spent)}</p>
      </div>
      <div data-testid="stats-change" class={figure}>
        <p class={sectionHeadingClass}>{$t('stats.change')}</p>
        {#if s.changePct !== null}
          <p class={figureValue}>{pct(s.changePct)}</p>
          <p class={line}>{$t('stats.vs', { period: period(s) })} · {signedMoney(s.spent - s.previous.cents)}</p>
        {:else}
          <p class={figureValue} aria-hidden="true">–</p>
          <p class={line}>{$t('stats.vs-none', { year: s.previous.year })}</p>
        {/if}
      </div>
      <div data-testid="stats-top" class={`relative isolate ${figure}`}>
        <p class={sectionHeadingClass}>{$t('stats.top-object')}</p>
        {#if s.top}
          {@const top = s.top}
          <button data-slot="stats-top-open" class={open} onclick={() => go(`/objects/${top.id}`)}>{top.name}</button>
          <p class={line}>{fmt(top.cents)} · {sharePct(top.cents, s.spent)}%</p>
        {:else}
          <p class={figureValue}><span aria-hidden="true">–</span><span class="sr-only">{$t('stats.top-none')}</span></p>
        {/if}
      </div>
    </section>
  {/if}

  {#if error}
    <p role="alert" class="m-0 mb-4 text-sm font-medium text-destructive">{error}</p>
  {:else if !data}
    <p class="m-0 mb-4 text-sm text-muted-foreground">{$t('nav.loading')}</p>
  {:else if data.total_cents === 0}
    <div class="flex flex-col items-center gap-3 px-4 py-10 text-center">
      <p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('stats.none')}</p>
    </div>
  {:else}
    <div class="mb-6 grid grid-cols-1 gap-4 wide:grid-cols-2">
      <section data-testid="stats-over-time" class={`${panel} wide:col-span-2`}>
        <div class="flex flex-wrap items-baseline justify-between gap-x-3">
          <h2 class={heading}>{$t('stats.over-time')}</h2>
          <p data-testid="stats-total" class="m-0 text-sm text-muted-foreground">{$t('stats.total')}: <b class={`${strong} tabular-nums`}>{fmt(data.total_cents)}</b></p>
        </div>
        <!-- Empty months (or years) are left out; each bar keeps its own label, so a gap shows. -->
        <Chart label={$t('stats.over-time')} items={data.over_time.map((a) => bar(a.bucket, a.cost_cents, fmt(a.cost_cents)))} />
      </section>
      <section data-testid="stats-by-object" class={`${panel} wide:col-span-2`}>
        <h2 class={heading}>{$t('stats.by-object')}</h2>
        <BarList items={objectBars} labelClass="w-32 desk:w-44 wide:w-64" />
      </section>
      <section data-testid="stats-by-type" class={panel}>
        <h2 class={heading}>{$t('stats.by-type')}</h2>
        <BarList items={bars(data.by_type, (b) => typeLabel(b, $customTypes, $t, $typesLoaded))} labelClass="w-28" />
      </section>
      <section data-testid="stats-by-category" class={panel}>
        <h2 class={heading}>{$t('stats.by-category')}</h2>
        <BarList items={bars(data.by_category, (b) => (b === PURCHASE_PRICE ? $t('stats.purchase-price') : $t(`cat.${b}`)))} labelClass="w-28" />
      </section>
    </div>
  {/if}

  <!-- Resources after money (round 0 fixed what the fuel section counts). They take neither the
       year nor the purchases switch: always the last twelve months. -->
  <div class="grid grid-cols-1 gap-4 wide:grid-cols-2">
    {#if energy && hasKwh}
      <section data-testid="stats-energy" class={panel}>
        <h2 class={heading}>{$t('stats.energy-title')}</h2>
        <p class={line}>
          {$t('stats.energy-current')}: <b class={strong}>{fmtKwh(energy.current_kwh_milli)}</b> · {$t('stats.energy-previous')}: {fmtKwh(energy.previous_kwh_milli)}{#if energy.target_kwh_milli > 0} · {$t('stats.target')}: {fmtKwh(energy.target_kwh_milli)}{/if}
        </p>
        <Chart label={$t('stats.energy-title')} items={energy.months.map((m) => bar(m.month, m.kwh_milli, fmtKwh(m.kwh_milli)))} />
      </section>
    {/if}
    {#if fuel && hasFuel}
      <section data-testid="stats-fuel" class={panel}>
        <h2 class={heading}>{$t('stats.fuel-title')}</h2>
        {#if hasLiters || hasGallons}
          <p class={line}>
            {$t('stats.energy-current')}: <b class={strong}>{[hasLiters ? fmtLiters(fuel.current_liters_milli) : '', hasGallons ? fmtGallons(fuel.current_gallons_milli) : ''].filter(Boolean).join(' · ')}</b>
            · {$t('stats.energy-previous')}: {[hasLiters ? fmtLiters(fuel.previous_liters_milli) : '', hasGallons ? fmtGallons(fuel.previous_gallons_milli) : ''].filter(Boolean).join(' · ')}
          </p>
        {/if}
        {#if fuel.levels.length > 0}
          <h3 class={sectionHeadingClass}>{$t('stats.fuel-levels')}</h3>
          <ul role="list" class="m-0 grid list-none grid-cols-1 gap-2 p-0 sm:grid-cols-2">
            {#each fuel.levels as level (level.object_id)}
              <li class="relative isolate flex flex-col gap-1 rounded-lg border border-border bg-background p-3">
                <div class="flex items-baseline justify-between gap-2">
                  <button data-slot="level-open" class={openLevel} onclick={() => go(`/objects/${level.object_id}`)}>{level.object_name}</button>
                  <b class="shrink-0 font-semibold text-foreground tabular-nums">{level.level_pct}%{#if level.remaining_milli !== null} · {level.unit === 'l' ? fmtLiters(level.remaining_milli) : fmtGallons(level.remaining_milli)}{/if}</b>
                </div>
                <p class={line}>
                  {periodLabel(level.date, $locale)}{#if level.estimated_days_remaining !== null} · {$t('stats.fuel-days', { n: level.estimated_days_remaining })}{/if}{#if level.low} · <span class="font-medium text-warn">{$t('stats.fuel-low')}</span>{/if}
                </p>
              </li>
            {/each}
          </ul>
        {/if}
        {#if hasLiters}
          <h3 class={sectionHeadingClass}>{$t('stats.fuel-liters')}</h3>
          <Chart label={$t('stats.fuel-liters')} items={fuel.months.map((m) => bar(m.month, m.liters_milli, fmtLiters(m.liters_milli)))} />
        {/if}
        {#if hasGallons}
          <h3 class={sectionHeadingClass}>{$t('stats.fuel-gallons')}</h3>
          <Chart label={$t('stats.fuel-gallons')} items={fuel.months.map((m) => bar(m.month, m.gallons_milli, fmtGallons(m.gallons_milli)))} />
        {/if}
      </section>
    {/if}
    {#if water && hasWater}
      <section data-testid="stats-water" class={panel}>
        <h2 class={heading}>{$t('stats.water-title')}</h2>
        <p class={line}>
          {$t('stats.energy-current')}: <b class={strong}>{fmtWater(water.current_liters_milli)}</b> · {$t('stats.energy-previous')}: {fmtWater(water.previous_liters_milli)} · {$t('water.daily-average')}: {fmtWater(water.daily_average_liters_milli)}{#if water.current_cost_cents > 0} · {$t('activity.cost')}: {fmt(water.current_cost_cents)}{/if}
        </p>
        {#if water.anomalies > 0}<p class="m-0 text-sm text-warn">{$t('water.anomalies', { n: water.anomalies })}</p>{/if}
        <Chart label={$t('stats.water-title')} items={water.months.map((m) => bar(m.month, m.liters_milli, `${fmtWater(m.liters_milli)}${m.estimated ? ` · ${$t('water.estimated-short')}` : ''}`))} />
        {#if water.objects.length > 1}
          <h3 class={sectionHeadingClass}>{$t('stats.by-object')}</h3>
          <BarList labelClass="w-36" items={water.objects.map((o) => ({ key: o.object_id, label: o.object_name, value: o.liters_milli, display: o.target_liters_milli ? `${fmtWater(o.liters_milli)} / ${fmtWater(o.target_liters_milli)}` : fmtWater(o.liters_milli) }))} />
        {/if}
      </section>
    {/if}
  </div>
</main>
