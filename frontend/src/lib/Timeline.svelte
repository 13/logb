<script lang="ts">
  import { formatWeight } from './weight';
  import type { WeightUnit } from './types';
  import { fileUrl } from './api';
  import { go } from './router';
  import { counter, fmtDate, money, quantity } from './format';
  import { dateFormat } from '../stores/date-format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import { activityTitle, groupByYear } from './activity-form';
  import { energyCost, energyLabelKey, fuelUnitLabel } from './energy';
  import { foldReadings, readingSpan } from './timeline-fold';
  import { placesLabel, spanLabel, tripDistance, formatDuration } from './trip';
  import { categoriesFor, customTypes } from './type-registry';
  import { CATEGORIES, type Activity, type Category, type CounterUnit, type ResourceUnit, type ObjectType } from './types';
  import { Button } from '$lib/components/ui/button/index.js';
  import CategoryIcon from './CategoryIcon.svelte';
  import Icon from './Icon.svelte';
  import TagChips from './TagChips.svelte';
  import { tagColorIndex } from './tags';

  let {
    objectId, type, activities, total, weightUnit = 'kg', loadingMore = false, onmore, onlog, ontriplog, onchargelog, unit, fuelUnit = null, resourceKind = null, energyRate = null,
    category = $bindable(''), tagFilter = $bindable(null), titleFilter = $bindable(null),
  }:
    {
      weightUnit?: WeightUnit; objectId: number; type: ObjectType; activities: Activity[]; total: number; loadingMore?: boolean;
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
      fuelUnit?: ResourceUnit;
      resourceKind?: string | null;
      /** The object's `cost_per_counter_milli` (see EnergyOut), loaded by ObjectDetail alongside
       *  the Info tab's Energy section rather than fetched here per row; `null` when it is not
       *  known yet, or genuinely not computable, in which case a trip row shows no cost at all. */
      energyRate?: number | null;
    } = $props();
  /** "energy.charged"/"energy.filled", picking the empty-state log button's wording. */
  const chargeStem = $derived(energyLabelKey(fuelUnit));
  // Folded here, once per change of the list, not in the template -- where it ran again on every
  // re-render of the year's block (a fold opened or closed, say).
  const groups = $derived(groupByYear(activities).map(([year, items]) => [year, foldReadings(items)] as const));
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
    [...categoriesFor(type, $customTypes, undefined, unit, fuelUnit, resourceKind), ...CATEGORIES.filter((c) => present.has(c))]
      .filter((c, i, all) => all.indexOf(c) === i),
  );
  /** Which folded runs of readings are open. */
  let open = $state<string[]>([]);
  function toggle(key: string) {
    open = open.includes(key) ? open.filter((k) => k !== key) : [...open, key];
  }

  /** The entry's title: a weight entry is its weight, anything else its title or its category's
   *  fallback wording. */
  const title = (a: Activity) => (a.weight_grams != null ? formatWeight(a.weight_grams, weightUnit, $locale) : activityTitle(a.title, a.category, $t, fuelUnit));

  /** The line under an entry's title: the date, then what the entry measured, joined by " · ".
   *  - a trip: the counter span it covered, its length, and its estimated energy cost (never
   *    stored; the "≈" tells it apart from a recorded amount);
   *  - a fill or charge: the counter, "full", the amount, a meter reading, "estimated", the level;
   *  - anything else: the counter.
   *  The amount paid is not here: it stands at the right of the title, where a column of entries
   *  lines their amounts up. */
  function meta(a: Activity): string {
    const q = (m: number | null | undefined) => quantity(m, fuelUnit ? fuelUnitLabel(fuelUnit) : null, $locale);
    const parts: string[] = [fmtDate(a.date, $dateFormat)];
    if (a.category === 'trip') {
      const dist = tripDistance(a);
      parts.push(spanLabel(counter(a.start_counter, unit, $locale), counter(a.counter_value, unit, $locale)));
      if (dist !== null) {
        parts.push(counter(dist, unit, $locale));
        if (energyRate !== null) parts.push(`≈ ${energyCost(dist, energyRate, $currency, $locale)}`);
      }
      return parts.join(' · ');
    }
    if (a.counter_value !== null) parts.push(counter(a.counter_value, unit, $locale));
    if (a.category === 'fuel' || a.category === 'usage') {
      if (a.charged_full) parts.push($t('energy.full'));
      if (a.quantity_milli !== null) parts.push(q(a.quantity_milli));
      if (a.meter_reading_milli != null) parts.push(`${$t('water.meter-reading')}: ${q(a.meter_reading_milli)}`);
      if (a.estimated) parts.push($t('water.estimated-short'));
      if (a.fuel_level_pct != null) parts.push($t('activity.fuel-level-value', { pct: a.fuel_level_pct }));
    }
    return parts.join(' · ');
  }

  /** The chip row fades at its right edge only while there is more to scroll to. */
  let chipRow = $state<HTMLDivElement | null>(null);
  $effect(() => {
    const el = chipRow;
    chipCategories;
    if (!el) return;
    const measure = () => {
      if (el.scrollWidth - el.clientWidth - el.scrollLeft > 1) el.setAttribute('data-fade', ''); else el.removeAttribute('data-fade');
    };
    measure();
    el.addEventListener('scroll', measure, { passive: true });
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    if (el.firstElementChild) ro.observe(el.firstElementChild);
    return () => { el.removeEventListener('scroll', measure); ro.disconnect(); };
  });
  const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring';
  /** Stretched link, as on the object cards: the title's ::after covers the entry. */
  const stretched = "after:absolute after:inset-0 after:rounded-lg after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-solid focus-visible:after:outline-offset-2 focus-visible:after:outline-ring";
  const readingCls = 'flex min-h-11 w-full cursor-pointer items-baseline justify-between gap-2 rounded-md border border-dashed border-border bg-transparent px-3 py-2 text-left text-sm text-foreground disabled:cursor-default disabled:opacity-60';
</script>

{#snippet readingRow(a: Activity)}
  <!-- A reading is one number, so it is one line, not a card. Still a button, so a typo can be
       opened and fixed like any other entry. -->
  <button data-slot="timeline-reading" data-testid="timeline-reading" disabled={a.pending} class={`${readingCls} ${focus}`}
          onclick={() => go(`/objects/${objectId}/activities/${a.id}`)}>
    <span class="text-muted-foreground">{fmtDate(a.date, $dateFormat)} · {$t('cat.reading')}{#if a.pending} · {$t('timeline.pending')}{/if}</span>
    <span class="tabular-nums">{counter(a.counter_value, unit, $locale)}</span>
  </button>
{/snippet}

{#snippet chip(value: Category | '', label: string)}
  <!-- 36 px to look at, 44 px to hit: the ::before adds 4 px above and below, inside the
       scroller's padding so it is not clipped. -->
  <button data-slot="filter-chip" aria-pressed={category === value} onclick={() => (category = value)}
          class={["relative inline-flex h-9 shrink-0 cursor-pointer items-center whitespace-nowrap rounded-full border px-3 text-sm font-medium transition-colors before:absolute before:inset-x-0 before:-inset-y-1 before:content-['']", focus,
                  category === value ? 'border-transparent bg-primary text-primary-foreground' : 'border-border bg-card text-foreground hover:bg-muted']}>{label}</button>
{/snippet}

<!-- The chips are one row that scrolls sideways; the fade at the right edge (data-fade, set while
     the row overflows and is not at its end) says there is more.
     The 4 px padding is the focus ring's bleed (2 px outline + 2 px offset), which the scroller
     would otherwise clip, and the fade is dropped while a chip has focus so it does not dim the
     ring. -->
<div data-testid="category-chips" class="-mx-1 mb-3">
  <div bind:this={chipRow} class="flex gap-2 overflow-x-auto p-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden data-[fade]:[mask-image:linear-gradient(to_right,#000_calc(100%_-_24px),transparent)] data-[fade]:focus-within:[mask-image:none]">
    {@render chip('', $t('timeline.filter-all'))}
    {#each chipCategories as c (c)}{@render chip(c, $t(`cat.${c}`))}{/each}
  </div>
</div>

{#if tagFilter !== null}
  <div data-testid="timeline-filter" class="mb-3 flex flex-wrap items-center gap-2">
    <span class={`tag tag-${tagColorIndex(tagFilter)}`}>{$t('tags.filter', { tag: tagFilter })}</span>
    <Button variant="outline" class="min-h-11" onclick={() => (tagFilter = null)}>{$t('tags.clear')}</Button>
  </div>
{/if}

{#if titleFilter !== null}
  <!-- Same pattern as the tag filter, combinable with it and with the category: this one narrows
       by exact title (a "Last done" row tapped in the details). `titleFilter` stays the real value
       the server matches on; only the chip's text falls back for an untitled entry. -->
  <div data-testid="timeline-filter" class="mb-3 flex flex-wrap items-center gap-2">
    <span class="rounded-full bg-muted px-3 py-1 text-sm text-foreground">{$t('lastdone.filter', { title: activityTitle(titleFilter, undefined, $t) })}</span>
    <Button variant="outline" class="min-h-11" onclick={() => (titleFilter = null)}>{$t('lastdone.clear')}</Button>
  </div>
{/if}

{#if activities.length === 0}
  <!-- An object with no history and an object whose filter matched nothing are not the same
       screen: the first is an invitation, the second is a fact about the chips above. -->
  <div class="flex flex-col items-center gap-3 px-4 py-10 text-center">
    {#if category === '' && tagFilter === null && titleFilter === null}
      <span class="text-muted-foreground opacity-40"><Icon name="edit" size={40} /></span>
      <p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('timeline.empty')}</p>
      {#if onlog}<Button class="min-h-11" onclick={() => onlog()}>+ {$t('timeline.log')}</Button>{/if}
      {#if ontriplog}<Button variant="outline" class="min-h-11" onclick={() => ontriplog()}>+ {$t('trip.log')}</Button>{/if}
      {#if onchargelog}<Button variant="outline" class="min-h-11" onclick={() => onchargelog()}>+ {resourceKind === 'water' ? $t('water.log') : $t(`${chargeStem}-log`)}</Button>{/if}
    {:else}
      <p class="m-0 text-sm text-muted-foreground">{$t('timeline.none-in-filter')}</p>
    {/if}
  </div>
{:else}
  {#each groups as [year, rows], gi (year)}
    <h2 class={['mb-2 text-sm font-semibold text-muted-foreground tabular-nums', gi === 0 ? 'mt-1' : 'mt-5']}>{year}</h2>
    <ul role="list" class="m-0 flex list-none flex-col gap-2 p-0">
      {#each rows as row (row.kind === 'entry' ? row.activity.id : row.key)}
        {#if row.kind === 'readings'}
          {@const span = readingSpan(row.readings)}
          {@const expanded = open.includes(row.key)}
          <!-- A run of readings between two real entries says one thing -- the counter went from
               here to there -- so it is one line until someone asks for the detail. -->
          <li>
            <button data-slot="timeline-fold" data-testid="timeline-fold" aria-expanded={expanded} onclick={() => toggle(row.key)} class={`${readingCls} ${focus}`}>
              <span class="text-muted-foreground">
                <span class="inline-block w-4" aria-hidden="true">{expanded ? '▾' : '▸'}</span>
                {fmtDate(row.readings[row.readings.length - 1].date, $dateFormat)} – {fmtDate(row.readings[0].date, $dateFormat)}
                · {$t('timeline.readings', { n: row.readings.length })}
              </span>
              {#if span}<span class="tabular-nums">{counter(span.from, unit, $locale)} – {counter(span.to, unit, $locale)}</span>{/if}
            </button>
            {#if expanded}
              <ul role="list" class="m-0 mt-2 flex list-none flex-col gap-2 p-0 pl-4">
                {#each row.readings as a (a.id)}<li>{@render readingRow(a)}</li>{/each}
              </ul>
            {/if}
          </li>
        {:else if row.activity.category === 'reading'}
          {@const a = row.activity}
          <!-- A single reading shows its chips like any entry; a folded run stays one line each. -->
          <li class="flex flex-col gap-1">
            {@render readingRow(a)}
            {#if (a.tags ?? []).length > 0}
              <div class="w-fit pl-3"><TagChips tags={a.tags} onselect={(tag) => (tagFilter = tag)} active={tagFilter} /></div>
            {/if}
          </li>
        {:else}
          {@const a = row.activity}
          <li data-testid="timeline-entry"
              class={['relative flex gap-3 rounded-lg border border-border bg-card p-3 shadow-xs transition-colors hover:border-input', a.pending && 'opacity-60']}>
            <span role="img" aria-label={$t(`cat.${a.category}`)} class="grid size-10 shrink-0 place-items-center rounded-md bg-primary/10 text-brand-ink">
              <CategoryIcon category={a.category} />
            </span>
            <div class="flex min-w-0 flex-1 flex-col gap-0.5">
              <div class="flex items-baseline gap-2">
                <button data-slot="entry-open" disabled={a.pending} onclick={() => go(`/objects/${objectId}/activities/${a.id}`)}
                        class={`min-w-0 flex-1 cursor-pointer break-words text-left font-semibold text-foreground disabled:cursor-default ${stretched}`}>{title(a)}</button>
                {#if a.cost_cents !== null}<span class="shrink-0 font-semibold text-foreground tabular-nums">{money(a.cost_cents, $currency, $locale)}</span>{/if}
              </div>
              <p class="m-0 text-sm text-muted-foreground tabular-nums">{meta(a)}</p>
              {#if a.category === 'trip' && placesLabel(a.from_place, a.to_place)}
                <p class="m-0 text-sm text-muted-foreground">{placesLabel(a.from_place, a.to_place)}</p>
              {/if}
              {#if a.category === 'trip' && (a.duration_minutes !== null || a.battery_used_pct !== null)}
                <p class="m-0 text-sm text-muted-foreground tabular-nums">
                  {[
                    a.duration_minutes !== null ? `${formatDuration(a.duration_minutes)} h` : null,
                    a.battery_used_pct !== null ? `${a.battery_used_pct} %` : null,
                  ].filter((s) => s !== null).join(' · ')}
                </p>
              {/if}
              {#if a.pending}<span class="w-fit rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('timeline.pending')}</span>{/if}
              {#if a.notes}<p class="m-0 mt-1 whitespace-pre-wrap text-sm text-foreground">{a.notes}</p>{/if}
              {#if a.attachments.length > 0}
                <!-- Wraps rather than scrolls: the stretched target covers this row, so a sideways
                     swipe here would never reach a scroller. -->
                <div data-testid="entry-thumbs" class="mt-1 flex flex-wrap gap-2">
                  {#each a.attachments.slice(0, 6) as att (att.id)}
                    {#if att.kind === 'photo'}
                      <img class="block size-16 shrink-0 rounded-md object-cover" src={fileUrl(att.file_id, true)} alt="" loading="lazy" decoding="async" />
                    {:else}
                      <span class="grid size-16 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground"><Icon name="document" size={28} /></span>
                    {/if}
                  {/each}
                </div>
              {/if}
              {#if (a.tags ?? []).length > 0}
                <!-- The chips are buttons of their own, raised above the stretched target. -->
                <div class="relative z-10 mt-1 w-fit"><TagChips tags={a.tags} onselect={(tag) => (tagFilter = tag)} active={tagFilter} /></div>
              {/if}
            </div>
          </li>
        {/if}
      {/each}
    </ul>
  {/each}
  {#if hasMore}
    <Button variant="outline" class="mt-3 min-h-11 w-full" onclick={() => onmore?.()} disabled={loadingMore}>
      {loadingMore ? $t('nav.loading') : $t('timeline.more', { n: total - activities.length })}
    </Button>
  {/if}
{/if}
