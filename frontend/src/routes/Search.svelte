<script lang="ts">
  import { errorMessage } from '../lib/api-error';
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
  import SearchIcon from '@lucide/svelte/icons/search';
  import { Button } from '$lib/components/ui/button/index.js';
  import { controlClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import CategoryIcon from '../lib/CategoryIcon.svelte';
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
  let offset = $state(0);
  let loadingMore = $state(false);
  const PAGE_SIZE = 25;

  // One request per pause in typing, and the query stays in the URL so a result list
  // survives a reload or a share.
  /** Writes the query into the address, replacing the entry only when it changed. */
  function remember(term: string) {
    const url = new URL(location.href);
    if (term) url.searchParams.set('q', term); else url.searchParams.delete('q');
    const next = url.pathname + url.search;
    if (next !== location.pathname + location.search) history.replaceState(null, '', next);
  }

  $effect(() => {
    const term = q.trim();
    const generation = ++requestGeneration;
    offset = 0;
    // Cleared at once; a term is written to the address with its request, once typing pauses,
    // not on every keystroke.
    if (!term) { remember(''); results = null; searched = ''; error = ''; loading = false; return; }
    // Aborted when a newer term (or leaving the page) supersedes it: the server stops working on
    // an answer nobody will read.
    const abort = new AbortController();
    const timer = setTimeout(async () => {
      remember(term);
      loading = true; error = '';
      try {
        const next = await api<SearchResults>('GET', `/search?q=${encodeURIComponent(term)}&offset=0`, undefined, undefined, { signal: abort.signal });
        if (generation !== requestGeneration) return;
        results = next; searched = term;
      } catch (e) {
        if (generation === requestGeneration) error = errorMessage(e, $t);
      } finally {
        if (generation === requestGeneration) loading = false;
      }
    }, 200);
    return () => { clearTimeout(timer); abort.abort(); };
  });

  async function loadMore() {
    if (!results?.has_more || loadingMore || !searched) return;
    loadingMore = true;
    const generation = requestGeneration;
    const nextOffset = offset + PAGE_SIZE;
    try {
      const next = await api<SearchResults>('GET', `/search?q=${encodeURIComponent(searched)}&offset=${nextOffset}`);
      if (generation !== requestGeneration || !results || searched === '') return;
      results = { objects: [...results.objects, ...next.objects], activities: [...results.activities, ...next.activities], has_more: next.has_more };
      offset = nextOffset;
    } catch (e) { error = errorMessage(e, $t); }
    finally { loadingMore = false; }
  }

  const empty = $derived(results !== null && results.objects.length === 0 && results.activities.length === 0);

  // The dashboard's object card (ObjectCard.svelte): one box, an icon tile, the name as the
  // button that opens it, stretched over the whole card; tag chips are raised above it.
  const card = 'relative isolate flex gap-3 rounded-lg border border-border bg-card p-3 shadow-xs transition-colors hover:border-input';
  const tile = 'grid size-12 shrink-0 place-items-center rounded-md bg-primary/10 text-brand-ink';
  const open = "min-w-0 cursor-pointer line-clamp-2 break-words text-left text-base font-semibold text-foreground after:absolute after:inset-0 after:rounded-lg after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-solid focus-visible:after:outline-offset-2 focus-visible:after:outline-ring";
  const facts = 'm-0 text-sm text-muted-foreground tabular-nums';
</script>

<main>
  <TopBar title={$t('search.title')} backTo="/" />

  <!-- mt-1: room for the focus ring below the sticky top bar. -->
  <div class="relative mt-1 mb-4">
    <SearchIcon aria-hidden="true" class="pointer-events-none absolute top-1/2 left-3 size-5 -translate-y-1/2 text-muted-foreground" />
    <!-- svelte-ignore a11y_autofocus -->
    <input type="search" data-slot="search" autofocus aria-label={$t('search.placeholder')} placeholder={$t('search.placeholder')}
           bind:value={q} class={`${controlClass} appearance-none pl-10`} />
  </div>

  {#if error}<p role="alert" class="m-0 mb-3 text-sm font-medium text-destructive">{error}</p>{/if}
  {#if loading && !results}
    <p class="m-0 text-sm text-muted-foreground">{$t('nav.loading')}</p>
  {:else if empty}
    <!-- A report about a query, not an invitation: it names what was searched for, and there is
         no action to offer. Before anything is typed nothing is drawn at all. -->
    <div class="flex flex-col items-center gap-3 px-4 py-10 text-center">
      <p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('search.none', { q: searched })}</p>
    </div>
  {:else if results}
    {#if results.objects.length > 0}
      <section aria-labelledby="search-objects" class="mb-6 flex flex-col gap-2">
        <h2 id="search-objects" class={sectionHeadingClass}>{$t('search.objects')}</h2>
        <ul role="list" class="m-0 grid list-none grid-cols-1 gap-3 p-0 wide:grid-cols-2">
          {#each results.objects as o (o.id)}
            <li data-testid="search-hit" class={card}>
              <span data-testid="hit-icon" class={tile} aria-hidden="true"><Icon name={typeIcon(o.type, $customTypes)} size={22} /></span>
              <div class="flex min-w-0 flex-1 flex-col gap-1">
                <button data-slot="hit-open" class={open} onclick={() => go(`/objects/${o.id}`)}>{o.name}</button>
                <p class={facts}>
                  {typeLabel(o.type, $customTypes, $t, $typesLoaded)}{o.parent_name ? ` · ${$t('search.in-parent', { name: o.parent_name })}` : ''}{o.archived_at ? ` · ${$t('search.archived')}` : ''}
                </p>
                {#if (o.tags ?? []).length > 0}
                  <!-- Plain labels: an object's own tag is rarely on its entries, so a tap would
                       open an empty timeline under a "Show entries tagged" label. -->
                  <div class="relative z-10 mt-1 w-fit"><TagChips tags={o.tags} /></div>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if results.activities.length > 0}
      <section aria-labelledby="search-activities" class="mb-6 flex flex-col gap-2">
        <h2 id="search-activities" class={sectionHeadingClass}>{$t('search.activities')}</h2>
        <ul role="list" class="m-0 grid list-none grid-cols-1 gap-3 p-0 wide:grid-cols-2">
          {#each results.activities as a (a.id)}
            <li data-testid="search-hit" class={card}>
              <span data-testid="hit-icon" class={tile} aria-hidden="true"><CategoryIcon category={a.category} size={22} /></span>
              <div class="flex min-w-0 flex-1 flex-col gap-1">
                <button data-slot="hit-open" class={open} onclick={() => go(`/objects/${a.object_id}/activities/${a.id}`)}>{activityTitle(a.title, a.category, $t)}</button>
                <p class={facts}>
                  {a.object_name} · {fmtDate(a.date, $dateFormat)}{a.cost_cents !== null ? ` · ${money(a.cost_cents, $currency, $locale)}` : ''}{a.weight_grams !== null ? ` · ${a.weight_grams} g` : ''}{placesLabel(a.from_place, a.to_place) ? ` · ${placesLabel(a.from_place, a.to_place)}` : ''}
                </p>
                {#if (a.tags ?? []).length > 0}
                  <!-- A tapped chip opens the entry's object with its timeline narrowed to that tag. -->
                  <div class="relative z-10 mt-1 w-fit">
                    <TagChips navigates tags={a.tags} onselect={(tag) => go(`/objects/${a.object_id}?tag=${encodeURIComponent(tag)}`)} />
                  </div>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
    {#if results.has_more}
      <Button variant="outline" class="min-h-11 w-full" disabled={loadingMore} onclick={loadMore}>{loadingMore ? $t('nav.loading') : $t('search.more')}</Button>
    {/if}
  {/if}
</main>
