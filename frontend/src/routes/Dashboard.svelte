<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import ObjectCard from '../lib/ObjectCard.svelte';
  import { api } from '../lib/api';
  import { go } from '../lib/router';
  import { locale, t } from '../i18n';
  import { fmtDate } from '../lib/format';
  import { persisted } from '../stores/persisted';
  import { SORT_KEYS, parseSort, parseTab, visibleRows, type ListTab, type SortKey } from '../lib/object-list';
  import type { MemObject, ObjectType, Reminder } from '../lib/types';
  import { tagColorIndex } from '../lib/tags';
  import Icon from '../lib/Icon.svelte';

  let active = $state<MemObject[]>([]);
  let archived = $state<MemObject[]>([]);
  let due = $state<Reminder[]>([]);
  let soon = $state<Reminder[]>([]);
  let loading = $state(true);
  let error = $state('');

  /** Per device, like the other view preferences; the address wins when it names a sort. */
  const rememberedSort = persisted<string>('logb.objects.sort', 'name');
  const params = new URLSearchParams(location.search);
  let tab = $state<ListTab>(parseTab(params.get('tab')));
  let sort = $state<SortKey>(parseSort(params.get('sort')) ?? parseSort($rememberedSort) ?? 'name');
  /** Session-only: a search is a moment's question, not a way of looking at the list. */
  let query = $state('');
  /** Session-only too, and not in the address: set by tapping a chip on a card. */
  let tagFilter = $state<string | null>(null);

  async function load() {
    loading = true; error = '';
    try {
      // Both tabs at every depth, once: switching tabs, searching and sorting then need no
      // request, and a search can find an object inside another. A household has tens of objects.
      [active, archived] = await Promise.all([
        api<MemObject[]>('GET', '/objects?all=true&archived=false'),
        api<MemObject[]>('GET', '/objects?all=true&archived=true'),
      ]);
      const all = await api<Reminder[]>('GET', '/reminders/due?within_days=30');
      due = all.filter((r) => r.due);
      soon = all.filter((r) => !r.due);
    } catch (e) { error = (e as Error).message; } finally { loading = false; }
  }
  onMount(load);

  function setSort(next: SortKey) { sort = next; rememberedSort.set(next); }

  // Tab and sort stay in the address, defaults left out, replaced only when it changes.
  $effect(() => {
    const url = new URL(location.href);
    if (tab === 'active') url.searchParams.delete('tab'); else url.searchParams.set('tab', tab);
    if (sort === 'name') url.searchParams.delete('sort'); else url.searchParams.set('sort', sort);
    const next = url.pathname + url.search;
    if (next !== location.pathname + location.search) history.replaceState(null, '', next);
  });

  const typeLabel = (ty: ObjectType) => $t(`type.${ty}`);
  const rows = $derived(visibleRows(active, archived, tab, query, sort, typeLabel, $locale, tagFilter));
  const activeCount = $derived(visibleRows(active, archived, 'active', '', 'name', typeLabel, $locale).length);
  const nothingYet = $derived(active.length === 0 && archived.length === 0);

  async function snooze(r: Reminder) {
    try { await api('POST', `/reminders/${r.id}/snooze`, { days: 7 }); await load(); }
    catch (e) { error = (e as Error).message; }
  }
</script>

<main>
  <TopBar title={$t('dash.title')} />

  {#if due.length > 0}
    <div class="banner">
      <b>{due.length === 1 ? $t('dash.due-one') : $t('dash.due', { n: due.length })}</b>
      <ul>
        {#each due.slice(0, 5) as r (r.id)}
          <li>
            <a href={`/objects/${r.object_id}`} onclick={(e) => { e.preventDefault(); go(`/objects/${r.object_id}?tab=reminders`); }}>{r.object_name}: {r.title}</a>
            {#if r.kind === 'reading'}
              <!-- The whole job is one number, so it is one tap from here. -->
              <button class="ghost snooze" onclick={() => go(`/objects/${r.object_id}/reading`)}>{$t('reminder.record')}</button>
            {/if}
            <button class="ghost snooze" onclick={() => snooze(r)}>{$t('reminder.snooze')}</button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#if soon.length > 0}
    <div class="banner soon">
      <b>{$t('dash.upcoming')}</b>
      <ul>
        {#each soon.slice(0, 5) as r (r.id)}
          <li>
            <a href={`/objects/${r.object_id}`} onclick={(e) => { e.preventDefault(); go(`/objects/${r.object_id}?tab=reminders`); }}>{r.object_name}: {r.title}</a>
            <span class="muted">
              {#if r.days_until !== null}{r.days_until === 1 ? $t('dash.in-day') : $t('dash.in-days', { n: r.days_until })}{/if}
              {#if r.counter_until !== null && r.counter_unit} · {$t('dash.in-counter', { n: r.counter_until, unit: r.counter_unit })}{/if}
              {#if r.estimated_due_date} · {$t('dash.estimated', { date: fmtDate(r.estimated_due_date, $locale) })}{/if}
            </span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  <nav class="tabs" aria-label={$t('dash.title')}>
    <button class:active={tab === 'active'} aria-pressed={tab === 'active'} onclick={() => (tab = 'active')}>
      {$t('dash.tab-active')} <span class="count muted">{activeCount}</span>
    </button>
    <button class:active={tab === 'archived'} aria-pressed={tab === 'archived'} onclick={() => (tab = 'archived')}>
      {$t('dash.tab-archived')} <span class="count muted">{archived.length}</span>
    </button>
  </nav>

  {#if error}<p class="error">{error}</p>{/if}
  {#if loading}
    <p class="muted">{$t('nav.loading')}</p>
  {:else if tab === 'active' && nothingYet}
    <!-- The one screen in the app that can say what LogB is for: it is what a new user sees
         the moment setup finishes. -->
    <div class="empty">
      <span class="empty-icon"><Icon name="object" size={40} /></span>
      <p>{$t('dash.empty')}</p>
      <button class="primary" onclick={() => go('/objects/new')}>+ {$t('dash.new')}</button>
    </div>
  {:else if tab === 'archived' && archived.length === 0}
    <div class="empty"><p>{$t('dash.none-archived')}</p></div>
  {:else}
    <div class="controls">
      <input type="search" aria-label={$t('dash.search')} placeholder={$t('dash.search')} bind:value={query} />
      <label class="sort">
        <span>{$t('dash.sort')}</span>
        <select value={sort} onchange={(e) => setSort(parseSort((e.currentTarget as HTMLSelectElement).value) ?? 'name')}>
          {#each SORT_KEYS as key (key)}<option value={key}>{$t(`dash.sort-${key}`)}</option>{/each}
        </select>
      </label>
    </div>
    {#if tagFilter !== null}
      <div class="tag-filter">
        <span class={`tag tag-${tagColorIndex(tagFilter)}`}>{$t('tags.filter', { tag: tagFilter })}</span>
        <button class="ghost" onclick={() => (tagFilter = null)}>{$t('tags.clear')}</button>
      </div>
    {/if}
    {#if rows.length === 0}
      <p class="muted">{$t('dash.no-match', { q: query.trim() || (tagFilter ?? '') })}</p>
    {:else}
      <div class="list">
        {#each rows as row (row.object.id)}<ObjectCard object={row.object} parentName={row.parentName} ontag={(tag) => (tagFilter = tag)} activeTag={tagFilter} />{/each}
      </div>
    {/if}
  {/if}

  <!-- Hidden while the first-run empty state carries the same action. -->
  {#if !nothingYet || tab === 'archived'}
    <button class="primary fab" onclick={() => go('/objects/new')}>+ {$t('dash.new')}</button>
  {/if}
</main>

<style>
  .banner ul { margin: var(--space-2) 0 0 var(--space-4); }
  /* Upcoming is not overdue: a calm surface card, not the alarming red used for `due`. */
  .banner.soon { background: var(--surface); color: var(--text); border: 1px solid var(--border); }
  .banner.soon a { color: var(--text); }
  .snooze { font-size: var(--text-xs); padding: 2px var(--space-2); }
  .controls { display: flex; gap: var(--space-2); flex-wrap: wrap; align-items: center; margin-bottom: var(--space-3); }
  .controls input[type='search'] { flex: 1 1 12rem; }
  .sort { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-sm); }
  .tag-filter { display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; margin-bottom: var(--space-3); }
  .tag-filter button { font-size: var(--text-sm); }
  .tabs .count { margin-left: var(--space-1); font-weight: normal; }
</style>
