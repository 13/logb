<script lang="ts">
  import { fileUrl } from './api';
  import { go } from './router';
  import { counter, fmtDate, money } from './format';
  import { dateFormat } from '../stores/date-format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import { groupByYear } from './activity-form';
  import { foldReadings, readingSpan } from './timeline-fold';
  import { categoriesFor, customTypes } from './type-registry';
  import { CATEGORIES, type Activity, type Category, type CounterUnit, type ObjectType } from './types';
  import Icon from './Icon.svelte';
  import TagChips from './TagChips.svelte';
  import { tagColorIndex } from './tags';

  let { objectId, type, activities, total, loadingMore = false, onmore, onlog, unit, category = $bindable(''), tagFilter = $bindable(null) }:
    {
      objectId: number; type: ObjectType; activities: Activity[]; total: number; loadingMore?: boolean;
      onmore?: () => void; onlog?: () => void; unit: CounterUnit; category?: Category | ''; tagFilter?: string | null;
    } = $props();
  const groups = $derived(groupByYear(activities));
  const hasMore = $derived(activities.length < total);
  // The type's vocabulary, plus any category the loaded entries actually use. The second half
  // matters after a re-type: without it, an entry logged as `fuel` on an object that is now a
  // `body` has no chip and cannot be filtered to at all. `activities` is only the page(s)
  // loaded so far, not necessarily every entry the object has -- acceptable here since this is
  // presentation, not the source of truth for what exists.
  const present = $derived(new Set(activities.map((a) => a.category)));
  const chipCategories = $derived(
    [...categoriesFor(type, $customTypes), ...CATEGORIES.filter((c) => present.has(c))]
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

{#if activities.length === 0}
  <!-- An object with no history and an object whose filter matched nothing are not the same
       screen: the first is an invitation, the second is a fact about the chip above it. -->
  <div class="empty">
    {#if category === '' && tagFilter === null}
      <span class="empty-icon"><Icon name="edit" size={40} /></span>
      <p>{$t('timeline.empty')}</p>
      {#if onlog}<button class="primary" onclick={() => onlog()}>+ {$t('timeline.log')}</button>{/if}
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
              <b>{a.title}</b>
              <span class="chip">{$t(`cat.${a.category}`)}</span>
              {#if a.pending}<span class="chip pending-chip">{$t('timeline.pending')}</span>{/if}
            </div>
            <div class="muted tnum">
              {fmtDate(a.date, $dateFormat)}
              {#if a.counter_value !== null} · {counter(a.counter_value, unit, $locale)}{/if}
              {#if a.cost_cents !== null} · {money(a.cost_cents, $currency, $locale)}{/if}
            </div>
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
