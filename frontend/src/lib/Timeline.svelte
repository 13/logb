<script lang="ts">
  import { fileUrl } from './api';
  import { go } from './router';
  import { counter, fmtDate, money, quantity } from './format';
  import { dateFormat } from '../stores/date-format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import { activityTitle, groupByYear } from './activity-form';
  import { energyCost, energyLabelKey } from './energy';
  import { foldReadings, readingSpan } from './timeline-fold';
  import { placesLabel, spanLabel, tripDistance, formatDuration } from './trip';
  import { categoriesFor, customTypes } from './type-registry';
  import { CATEGORIES, type Activity, type Category, type CounterUnit, type FuelUnit, type ObjectType } from './types';
  import Icon from './Icon.svelte';
  import TagChips from './TagChips.svelte';
  import { tagColorIndex } from './tags';

  let {
    objectId, type, activities, total, loadingMore = false, onmore, onlog, ontriplog, onchargelog, unit, fuelUnit = null, energyRate = null,
    category = $bindable(''), tagFilter = $bindable(null), titleFilter = $bindable(null),
  }:
    {
      objectId: number; type: ObjectType; activities: Activity[]; total: number; loadingMore?: boolean;
      onmore?: () => void; onlog?: () => void;
      /** Set only on a km/mi object (see ObjectDetail.svelte) -- offers "+ Log trip" in the
       *  empty state beside the plain "+ Log activity" one, the same pair the object page's own
       *  floating buttons offer once there is at least one entry. */
      ontriplog?: () => void;
      /** Set only on an object with a `fuel_unit` (see ObjectDetail.svelte) -- offers "+ Log
       *  charge"/"+ Log fill" in the empty state, the same way `ontriplog` offers a trip. */
      onchargelog?: () => void;
      unit: CounterUnit; category?: Category | '';
      tagFilter?: string | null; titleFilter?: string | null;
      /** The object's fuel unit, for a fuel row's amount and the empty-state log button's
       *  wording (`energyLabelKey`); `null` on an object with none. */
      fuelUnit?: FuelUnit;
      /** The object's `cost_per_counter_milli` (see EnergyOut), loaded by ObjectDetail alongside
       *  the Info tab's Energy section rather than fetched here per row; `null` when it is not
       *  known yet, or genuinely not computable, in which case a trip row shows no cost at all. */
      energyRate?: number | null;
    } = $props();
  /** "energy.charged"/"energy.filled", picking the empty-state log button's wording. */
  const chargeStem = $derived(energyLabelKey(fuelUnit));
  const groups = $derived(groupByYear(activities));
  const hasMore = $derived(activities.length < total);
  // The type's vocabulary, plus any category the loaded entries actually use. The second half
  // matters after a re-type: without it, an entry logged as `fuel` on an object that is now a
  // `body` has no chip and cannot be filtered to at all. `activities` is only the page(s)
  // loaded so far, not necessarily every entry the object has -- acceptable here since this is
  // presentation, not the source of truth for what exists.
  const present = $derived(new Set(activities.map((a) => a.category)));
  // `unit`: the same argument that makes `categoriesFor` offer `trip` on the entry form -- an
  // object with a km/mi counter gets a "Trip" filter chip even before it has logged one, exactly
  // like every other category chip here (offered by the type/unit, not only once something of
  // that kind exists).
  const chipCategories = $derived(
    [...categoriesFor(type, $customTypes, undefined, unit), ...CATEGORIES.filter((c) => present.has(c))]
      .filter((c, i, all) => all.indexOf(c) === i),
  );
  /** Which folded runs of readings are open. */
  let open = $state<string[]>([]);
  function toggle(key: string) {
    open = open.includes(key) ? open.filter((k) => k !== key) : [...open, key];
  }
</script>

{#snippet readingRow(a: Activity)}
  <!-- A reading is one number, so it is one line, not a card. Still a button, so a typo can be
       opened and fixed like any other entry. -->
  <button
    class="entry reading"
    class:pending={a.pending}
    disabled={a.pending}
    onclick={() => go(`/objects/${objectId}/activities/${a.id}`)}
  >
    <span class="muted">{fmtDate(a.date, $dateFormat)} · {$t('cat.reading')}{#if a.pending} · {$t('timeline.pending')}{/if}</span>
    <span class="tnum">{counter(a.counter_value, unit, $locale)}</span>
  </button>
{/snippet}

<div class="chips">
  <button class:active={category === ''} class="chip" onclick={() => (category = '')}>{$t('timeline.filter-all')}</button>
  {#each chipCategories as c}
    <button class:active={category === c} class="chip" onclick={() => (category = c)}>{$t(`cat.${c}`)}</button>
  {/each}
</div>

{#if tagFilter !== null}
  <div class="tag-filter">
    <span class={`tag tag-${tagColorIndex(tagFilter)}`}>{$t('tags.filter', { tag: tagFilter })}</span>
    <button class="ghost" onclick={() => (tagFilter = null)}>{$t('tags.clear')}</button>
  </div>
{/if}

{#if titleFilter !== null}
  <!-- Same chip-with-clear-button pattern as the tag filter above, combinable with it and with
       category: this one narrows by exact title (a "Last done" row tapped on the Info tab),
       not by tag. -->
  <div class="tag-filter">
    <!-- `titleFilter` itself stays the real (possibly empty, for an untitled trip) value the
         server matches on; only the chip's own text falls back to "Trip", same as every other
         place a trip's title is shown. -->
    <span class="chip">{$t('lastdone.filter', { title: activityTitle(titleFilter, $t) })}</span>
    <button class="ghost" onclick={() => (titleFilter = null)}>{$t('lastdone.clear')}</button>
  </div>
{/if}

{#if activities.length === 0}
  <!-- An object with no history and an object whose filter matched nothing are not the same
       screen: the first is an invitation, the second is a fact about the chip(s) above it. -->
  <div class="empty">
    {#if category === '' && tagFilter === null && titleFilter === null}
      <span class="empty-icon"><Icon name="edit" size={40} /></span>
      <p>{$t('timeline.empty')}</p>
      {#if onlog}<button class="primary" onclick={() => onlog()}>+ {$t('timeline.log')}</button>{/if}
      {#if ontriplog}<button class="ghost" onclick={() => ontriplog()}>+ {$t('trip.log')}</button>{/if}
      {#if onchargelog}<button class="ghost" onclick={() => onchargelog()}>+ {$t(`${chargeStem}-log`)}</button>{/if}
    {:else}
      <p>{$t('timeline.none-in-filter')}</p>
    {/if}
  </div>
{:else}
  {#each groups as [year, items] (year)}
    <p class="year">{year}</p>
    <div class="list">
      {#each foldReadings(items) as row (row.kind === 'entry' ? row.activity.id : row.key)}
        {#if row.kind === 'readings'}
          {@const span = readingSpan(row.readings)}
          {@const expanded = open.includes(row.key)}
          <!-- A run of readings between two real entries says one thing -- the counter went from
               here to there -- so it is one line until someone asks for the detail. -->
          <button class="entry reading fold" aria-expanded={expanded} onclick={() => toggle(row.key)}>
            <span class="muted">
              <span class="caret" aria-hidden="true">{expanded ? '▾' : '▸'}</span>
              {fmtDate(row.readings[row.readings.length - 1].date, $dateFormat)} – {fmtDate(row.readings[0].date, $dateFormat)}
              · {$t('timeline.readings', { n: row.readings.length })}
            </span>
            {#if span}<span class="tnum">{counter(span.from, unit, $locale)} – {counter(span.to, unit, $locale)}</span>{/if}
          </button>
          {#if expanded}
            <div class="list folded">
              {#each row.readings as a (a.id)}{@render readingRow(a)}{/each}
            </div>
          {/if}
        {:else if row.activity.category === 'reading'}
          {@const a = row.activity}
          <!-- A single reading shows its chips like any entry; a folded run stays one line each. -->
          <div class="entry-row">
          {@render readingRow(a)}
          {#if (a.tags ?? []).length > 0}
            <div class="entry-tags"><TagChips tags={a.tags} onselect={(tag) => (tagFilter = tag)} active={tagFilter} /></div>
          {/if}
          </div>
        {:else}
          {@const a = row.activity}
          <!-- The chips can be buttons, and a button cannot sit inside the entry's button, so they
               sit below it; `.entry-row` keeps the two together as one item of the list. -->
          <div class="entry-row">
          <button
            class="card entry"
            class:pending={a.pending}
            disabled={a.pending}
            onclick={() => go(`/objects/${objectId}/activities/${a.id}`)}
          >
            <div class="row head">
              <b>{activityTitle(a.title, $t)}</b>
              <span class="chip">{$t(`cat.${a.category}`)}</span>
              {#if a.pending}<span class="chip pending-chip">{$t('timeline.pending')}</span>{/if}
            </div>
            {#if a.category === 'trip'}
              {@const dist = tripDistance(a)}
              <!-- Start/end/distance instead of the single reading a plain counter value would
                   show below -- a trip moved the counter across a *span*, not to a bare number. -->
              <div class="muted tnum">
                {fmtDate(a.date, $dateFormat)} · {spanLabel(counter(a.start_counter, unit, $locale), counter(a.counter_value, unit, $locale))}
                {#if dist !== null} · {counter(dist, unit, $locale)}{/if}
                {#if a.cost_cents !== null} · {money(a.cost_cents, $currency, $locale)}{/if}
                <!-- Never stored, always an estimate -- the "≈" tells it apart from `cost_cents`
                     just before it, which is a real recorded amount. Nothing shown at all once
                     the rate (or the distance itself) is not known. -->
                {#if dist !== null && energyRate !== null} · ≈ {energyCost(dist, energyRate, $currency, $locale)}{/if}
              </div>
              {#if placesLabel(a.from_place, a.to_place)}
                <div class="muted">{placesLabel(a.from_place, a.to_place)}</div>
              {/if}
              {#if a.duration_minutes !== null || a.battery_used_pct !== null}
                <div class="muted">
                  {[
                    a.duration_minutes !== null ? `${formatDuration(a.duration_minutes)} h` : null,
                    a.battery_used_pct !== null ? `${a.battery_used_pct} %` : null,
                  ].filter((s) => s !== null).join(' · ')}
                </div>
              {/if}
            {:else if a.category === 'fuel'}
              <!-- Counter first (a charge/fill marks the odometer like any reading), then "full"
                   when it topped up, the amount, and the cost -- see the design spec's own
                   example row. -->
              <div class="muted tnum">
                {fmtDate(a.date, $dateFormat)}
                {#if a.counter_value !== null} · {counter(a.counter_value, unit, $locale)}{/if}
                {#if a.charged_full} · {$t('energy.full')}{/if}
                {#if a.quantity_milli !== null} · {quantity(a.quantity_milli, fuelUnit ?? (unit === 'mi' ? 'gal' : 'l'), $locale)}{/if}
                {#if a.cost_cents !== null} · {money(a.cost_cents, $currency, $locale)}{/if}
              </div>
            {:else}
              <div class="muted tnum">
                {fmtDate(a.date, $dateFormat)}
                {#if a.counter_value !== null} · {counter(a.counter_value, unit, $locale)}{/if}
                {#if a.cost_cents !== null} · {money(a.cost_cents, $currency, $locale)}{/if}
              </div>
            {/if}
            {#if a.notes}<p class="notes">{a.notes}</p>{/if}
            {#if a.attachments.length > 0}
              <div class="thumb-strip">
                {#each a.attachments.slice(0, 6) as att (att.id)}
                  {#if att.kind === 'photo'}<img src={fileUrl(att.file_id, true)} alt="" loading="lazy" />{:else}<span class="doc-chip"><Icon name="document" size={28} /></span>{/if}
                {/each}
              </div>
            {/if}
          </button>
          {#if (a.tags ?? []).length > 0}
            <div class="entry-tags"><TagChips tags={a.tags} onselect={(tag) => (tagFilter = tag)} active={tagFilter} /></div>
          {/if}
          </div>
        {/if}
      {/each}
    </div>
  {/each}
  {#if hasMore}
    <button class="more" onclick={() => onmore?.()} disabled={loadingMore}>
      {loadingMore ? $t('nav.loading') : $t('timeline.more', { n: total - activities.length })}
    </button>
  {/if}
{/if}

<style>
  .entry { display: flex; flex-direction: column; gap: var(--space-1); text-align: left; width: 100%; }
  .entry.pending { opacity: .55; cursor: default; }
  .entry.reading {
    flex-direction: row; justify-content: space-between; align-items: baseline; gap: var(--space-2);
    min-height: auto; padding: var(--space-2) var(--space-3);
    background: transparent; border: 1px dashed var(--border); border-radius: var(--radius-sm);
    font-size: var(--text-sm); color: var(--text);
  }
  .caret { display: inline-block; width: 1em; }
  .folded { padding-left: var(--space-4); }
  .pending-chip { flex: none; }
  .head { justify-content: space-between; }
  .head b { flex: 1; }
  .head .chip { flex: none; }
  .notes { font-size: var(--text-sm); white-space: pre-wrap; }
  .entry-tags { margin-top: var(--space-1); padding-left: var(--space-3); }
  .tag-filter { display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; margin-bottom: var(--space-3); }
  .tag-filter button { font-size: var(--text-sm); }
  .more { width: 100%; margin-top: var(--space-3); }
  .doc-chip { display: grid; place-items: center; width: 64px; height: 64px; background: var(--surface-2); border-radius: var(--radius-sm); }
</style>
