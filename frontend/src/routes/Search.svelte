<script lang="ts">
  import TopBar from '../lib/TopBar.svelte';
  import { api } from '../lib/api';
  import { go } from '../lib/router';
  import { fmtDate, money } from '../lib/format';
  import { activityTitle } from '../lib/activity-form';
  import { placesLabel } from '../lib/trip';
  import { dateFormat } from '../stores/date-format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { SearchResults } from '../lib/types';
  import { customTypes, typeIcon, typeLabel, typesLoaded } from '../lib/type-registry';
  import Icon from '../lib/Icon.svelte';
  import TagChips from '../lib/TagChips.svelte';

  let q = $state(new URLSearchParams(location.search).get('q') ?? '');
  let results = $state<SearchResults | null>(null);
  /** The term `results` actually describes. `q` changes on every keystroke and the request
   *  is debounced behind it, so quoting `q` in the no-matches line would name a query the
   *  server has not answered yet. */
  let searched = $state('');
  let loading = $state(false);
  let error = $state('');
  let requestGeneration = 0;

  // One request per pause in typing, and the query stays in the URL so a result list
  // survives a reload or a share.
  $effect(() => {
    const term = q.trim();
    const generation = ++requestGeneration;
    const url = new URL(location.href);
    if (term) url.searchParams.set('q', term); else url.searchParams.delete('q');
    history.replaceState(null, '', url.pathname + url.search);
    if (!term) { results = null; searched = ''; error = ''; loading = false; return; }
    const timer = setTimeout(async () => {
      loading = true; error = '';
      try {
        const next = await api<SearchResults>('GET', `/search?q=${encodeURIComponent(term)}`);
        if (generation !== requestGeneration) return;
        results = next; searched = term;
      } catch (e) {
        if (generation === requestGeneration) error = (e as Error).message;
      } finally {
        if (generation === requestGeneration) loading = false;
      }
    }, 200);
    return () => clearTimeout(timer);
  });

  const empty = $derived(results !== null && results.objects.length === 0 && results.activities.length === 0);
</script>

<main>
  <TopBar title={$t('search.title')} backTo="/" />

  <!-- svelte-ignore a11y_autofocus -->
  <input
    type="search"
    autofocus
    aria-label={$t('search.placeholder')}
    placeholder={$t('search.placeholder')}
    bind:value={q}
  />

  {#if error}<p class="error">{error}</p>{/if}
  {#if loading && !results}
    <p class="muted">{$t('nav.loading')}</p>
  {:else if empty}
    <!-- A report about a query, not an invitation: it names what was searched for, and there is
         no action to offer. Before anything is typed `results` is null and nothing is drawn at
         all -- "no matches" ahead of a query would be a claim about a search nobody ran. -->
    <div class="empty">
      <p>{$t('search.none', { q: searched })}</p>
    </div>
  {:else if results}
    {#if results.objects.length > 0}
      <h2>{$t('search.objects')}</h2>
      <div class="list">
        {#each results.objects as o (o.id)}
          <!-- The chips can be buttons, and a button cannot sit inside the hit's button, so they
               sit below it; `.hit-row` keeps the two together, as `.entry-row` does on the timeline. -->
          <div class="hit-row">
            <button class="hit" onclick={() => go(`/objects/${o.id}`)}>
              <span class="hit-title">{o.name}</span>
              <span class="muted small type-row">
                <Icon name={typeIcon(o.type, $customTypes)} size={14} />
                {typeLabel(o.type, $customTypes, $t, $typesLoaded)}{o.parent_name ? ` · ${$t('search.in-parent', { name: o.parent_name })}` : ''}{o.archived_at ? ` · ${$t('search.archived')}` : ''}
              </span>
            </button>
            {#if (o.tags ?? []).length > 0}
              <div class="hit-tags"><!-- Plain labels: an object's own tag is rarely on its entries, so a tap would open an empty
                   timeline under a "Show entries tagged" label. -->
              <TagChips tags={o.tags} /></div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
    {#if results.activities.length > 0}
      <h2>{$t('search.activities')}</h2>
      <div class="list">
        {#each results.activities as a (a.id)}
          <div class="hit-row">
            <button class="hit" onclick={() => go(`/objects/${a.object_id}/activities/${a.id}`)}>
              <span class="hit-title">{activityTitle(a.title, a.category, $t)}</span>
              <span class="muted small tnum">
                {a.object_name} · {fmtDate(a.date, $dateFormat)}{a.cost_cents !== null ? ` · ${money(a.cost_cents, $currency, $locale)}` : ''}{a.weight_grams !== null ? ` · ${a.weight_grams} g` : ''}{placesLabel(a.from_place, a.to_place) ? ` · ${placesLabel(a.from_place, a.to_place)}` : ''}
              </span>
            </button>
            {#if (a.tags ?? []).length > 0}
              <!-- A tapped chip opens the entry's object with its timeline narrowed to that tag. -->
              <div class="hit-tags"><TagChips navigates tags={a.tags} onselect={(tag) => go(`/objects/${a.object_id}?tag=${encodeURIComponent(tag)}`)} /></div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</main>

<style>
  /* Full width inside its `.hit-row`, as it was when it sat in the list directly. */
  .hit { display: flex; flex-direction: column; align-items: flex-start; gap: var(--space-1); text-align: left; background: var(--surface-2); width: 100%; }
  .hit-title { font-weight: 600; }
  .hit-tags { margin-top: var(--space-1); padding-left: var(--space-3); }
  .small { font-size: var(--text-xs); }
  .type-row { display: flex; align-items: center; gap: var(--space-1); }
  .type-row :global(svg) { flex: none; }
</style>
