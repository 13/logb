<script lang="ts">
  import TopBar from '../lib/TopBar.svelte';
  import { api } from '../lib/api';
  import { go } from '../lib/router';
  import { fmtDate, money } from '../lib/format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { SearchResults } from '../lib/types';
  import { customTypes, typeIcon, typeLabel, typesLoaded } from '../lib/type-registry';
  import Icon from '../lib/Icon.svelte';

  let q = $state(new URLSearchParams(location.search).get('q') ?? '');
  let results = $state<SearchResults | null>(null);
  /** The term `results` actually describes. `q` changes on every keystroke and the request
   *  is debounced behind it, so quoting `q` in the no-matches line would name a query the
   *  server has not answered yet. */
  let searched = $state('');
  let loading = $state(false);
  let error = $state('');

  // One request per pause in typing, and the query stays in the URL so a result list
  // survives a reload or a share.
  $effect(() => {
    const term = q.trim();
    const url = new URL(location.href);
    if (term) url.searchParams.set('q', term); else url.searchParams.delete('q');
    history.replaceState(null, '', url.pathname + url.search);
    if (!term) { results = null; searched = ''; error = ''; return; }
    const timer = setTimeout(async () => {
      loading = true; error = '';
      try { results = await api<SearchResults>('GET', `/search?q=${encodeURIComponent(term)}`); searched = term; }
      catch (e) { error = (e as Error).message; }
      finally { loading = false; }
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
          <button class="hit" onclick={() => go(`/objects/${o.id}`)}>
            <span class="hit-title">{o.name}</span>
            <span class="muted small type-row">
              <Icon name={typeIcon(o.type, $customTypes)} size={14} />
              {typeLabel(o.type, $customTypes, $t, $typesLoaded)}{o.parent_name ? ` · ${$t('search.in-parent', { name: o.parent_name })}` : ''}{o.archived_at ? ` · ${$t('search.archived')}` : ''}
            </span>
          </button>
        {/each}
      </div>
    {/if}
    {#if results.activities.length > 0}
      <h2>{$t('search.activities')}</h2>
      <div class="list">
        {#each results.activities as a (a.id)}
          <button class="hit" onclick={() => go(`/objects/${a.object_id}/activities/${a.id}`)}>
            <span class="hit-title">{a.title}</span>
            <span class="muted small tnum">
              {a.object_name} · {fmtDate(a.date, $locale)}{a.cost_cents !== null ? ` · ${money(a.cost_cents, $currency, $locale)}` : ''}
            </span>
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</main>

<style>
  .hit { display: flex; flex-direction: column; align-items: flex-start; gap: var(--space-1); text-align: left; background: var(--surface-2); }
  .hit-title { font-weight: 600; }
  .small { font-size: var(--text-xs); }
  .type-row { display: flex; align-items: center; gap: var(--space-1); }
  .type-row :global(svg) { flex: none; }
</style>
