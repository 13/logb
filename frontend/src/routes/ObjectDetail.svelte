<script lang="ts">
  import { untrack } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import Timeline from '../lib/Timeline.svelte';
  import Documents from '../lib/Documents.svelte';
  import Reminders from '../lib/Reminders.svelte';
  import Insights from '../lib/Insights.svelte';
  import Icon from '../lib/Icon.svelte';
  import ObjectCard from '../lib/ObjectCard.svelte';
  import TagChips from '../lib/TagChips.svelte';
  import { foldTag } from '../lib/tags';
  import { customTypes, typeIcon, typeLabel, typesLoaded } from '../lib/type-registry';
  import { api, apiPage, fileUrl, isRejection, onOutboxFlushed, pendingOpsFor, markServingSaved, supersedeStale } from '../lib/api';
  import { getCachedActivities, getCachedObject, setCachedActivities, setCachedObject } from '../lib/object-cache';
  import { go } from '../lib/router';
  import { counter, fmtDate, money } from '../lib/format';
  import { dateFormat } from '../stores/date-format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { Activity, ActivityInput, Category, MemObject } from '../lib/types';
  import { fetchWindow, mergeWindow, shouldReload, windowFor, type LoadMode } from '../lib/timeline-load';
  import type { QueuedOp } from '../lib/outbox';

  let { id }: { id: string } = $props();
  const oid = $derived(Number(id));
  type Tab = 'timeline' | 'documents' | 'reminders' | 'info';
  const initialQuery = new URLSearchParams(location.search);
  /** A `?tag=` link (a chip tapped in search) opens the timeline already narrowed to that tag.
   *  Read on mount only: the router's `path` store holds just the pathname, so a later change to
   *  the query string alone does not reach this component. */
  const initialTag = initialQuery.get('tag')?.trim() || null;
  let tab = $state<Tab>(initialTag !== null ? 'timeline' : (initialQuery.get('tab') as Tab) || 'timeline');
  let object = $state<MemObject | null>(null);
  let activities = $state<Activity[]>([]);
  /// How many activities match the current filter in total, page window aside.
  let activityTotal = $state(0);
  let loadingMore = $state(false);
  let category = $state<Category | ''>('');
  /** Session-only, like `category`: a tag tapped on an entry narrows the timeline to it. */
  let tagFilter = $state<string | null>(initialTag);
  let children = $state<MemObject[]>([]);
  /** Archived children are not listed under Contents, but their costs still count with "Include
   *  contents", so having any is enough to offer the switch. */
  let archivedChildCount = $state(0);
  let error = $state('');

  /** Only a genuine connectivity failure (see `isRejection`) may fall back to the cache — a
   *  401/403/404 is the server answering, and this object may simply belong to someone else. */
  async function loadObject() {
    try {
      object = await api<MemObject>('GET', `/objects/${oid}`);
      setCachedObject(oid, object);
      error = '';
    } catch (e) {
      const cached = isRejection(e) ? undefined : getCachedObject(oid);
      if (cached) object = cached;
      else error = (e as Error).message;
    }
  }

  /** The object's direct children, for the Contents section on the Info tab. Loaded only while
   *  that tab is open -- the other tabs have no use for it. */
  async function loadChildren() {
    try { children = await api<MemObject[]>('GET', `/objects?parent_id=${oid}&archived=false`); }
    catch (e) { error = (e as Error).message; }
    try { archivedChildCount = (await api<MemObject[]>('GET', `/objects?parent_id=${oid}&archived=true`)).length; }
    catch { archivedChildCount = 0; }
  }

  /** A stable negative id for a queued create, so it can sit in the same `id`-keyed list as
   *  real activities without colliding with one (real ids are always positive). */
  function pendingId(opId: string): number {
    let h = 0;
    for (let i = 0; i < opId.length; i++) h = (h * 31 + opId.charCodeAt(i)) | 0;
    return -(Math.abs(h) || 1);
  }

  /** A queued 'activity.create' has no server row yet, so it renders straight from what the
   *  form queued rather than from a GET — otherwise a log made underground would stay invisible
   *  until the phone gets signal back, which is exactly the failure this task exists to avoid.
   *  `pending: true` tells `Timeline` to dim it and refuse navigation into its (fake) id. */
  function pendingToActivity(op: QueuedOp): Activity {
    const b = op.body as Partial<ActivityInput>;
    return {
      id: pendingId(op.id), object_id: oid,
      date: typeof b.date === 'string' ? b.date : new Date().toISOString().slice(0, 10),
      category: (b.category as Category) ?? 'other',
      title: typeof b.title === 'string' ? b.title : '',
      notes: typeof b.notes === 'string' ? b.notes : '',
      counter_value: typeof b.counter_value === 'number' ? b.counter_value : null,
      cost_cents: typeof b.cost_cents === 'number' ? b.cost_cents : null,
      quantity_milli: typeof b.quantity_milli === 'number' ? b.quantity_milli : null,
      created_at: new Date().toISOString(), updated_at: new Date().toISOString(), attachments: [],
      pending: true, tags: Array.isArray(b.tags) ? b.tags : [],
    };
  }

  async function pendingActivities(): Promise<Activity[]> {
    const ops = await pendingOpsFor(`/objects/${oid}/activities`);
    // The server filters the loaded page by tag; a queued entry has not reached it, so it is
    // filtered here the same way (ignoring case and accents), or it would show under any tag.
    const wanted = tagFilter === null ? null : foldTag(tagFilter);
    return ops
      .filter((o) => !category || o.body.category === category)
      .filter((o) => wanted === null || (Array.isArray(o.body.tags) && (o.body.tags as string[]).some((x) => foldTag(x) === wanted)))
      .map(pendingToActivity);
  }

  /// Guards against two loads landing out of order: only the newest may commit its result.
  /// Several triggers can overlap (the `oid`/`category` effect, "load more", a flush), and
  /// whichever resolved last used to win regardless of which started last.
  let loadSeq = 0;
  // Leaving the page makes every in-flight load non-current, so a slow failure arriving after
  // the user moved on cannot raise the saved-data note over the screen they moved to.
  $effect(() => () => { loadSeq++; });

  /// The window arithmetic, the chunk loop and the merge live in ../lib/timeline-load.ts, where
  /// they are unit-testable; what stays here is the part that genuinely needs the component:
  /// reading and assigning its state, the offline-cache fallback, and the supersede check.
  async function loadActivities(mode: LoadMode = 'reset') {
    const token = ++loadSeq;
    const activitiesPrefix = `/objects/${oid}/activities?`;
    // A reset or refresh replaces the whole list under a new query string without a route change,
    // so staleness recorded for the list it replaces must not keep the saved-data note up.
    // Loading older entries keeps the rows already shown, and their staleness with them. Before
    // any await, so overlapping loads supersede in the order they started.
    if (mode !== 'append') supersedeStale(activitiesPrefix);
    // The offline cache holds one page per object: the unfiltered one. A filtered page written
    // there would later show as the whole timeline offline, and reading it back under a filter
    // would show unfiltered entries as if they matched -- so a filtered load neither writes nor
    // reads it.
    const filtered = category !== '' || tagFilter !== null;
    // `untrack`: this runs synchronously inside the `oid`/`category` $effect below, so reading
    // `activities` here made that effect depend on the very list it goes on to assign. The
    // effect then re-ran on its own result and started over from page one -- which is why
    // "Show N older" appeared to do nothing at all: the appended page was fetched, rendered,
    // and immediately replaced by a fresh page-one load.
    const loaded = untrack(() => activities.filter((a) => !a.pending).length);
    const { want, base, append } = windowFor(mode, loaded);
    const pending = append ? [] : await pendingActivities();
    let items: Activity[] = [];
    let total = 0;
    let fetched = false;
    try {
      const page = await fetchWindow<Activity>(want, base, (limit, offset) => {
        // A newer load has started: stop asking for this one's next chunks. Nobody will show
        // them, and an answer sent after the newer load superseded the old query would count
        // as staleness again and bring the stuck note back.
        if (token !== loadSeq) throw new Error('superseded');
        const params = new URLSearchParams({ limit: String(limit), offset: String(offset) });
        if (category) params.set('category', category);
        if (tagFilter) params.set('tag', tagFilter);
        return apiPage<Activity>(`${activitiesPrefix}${params}`);
      });
      items = page.items;
      total = page.total;
      fetched = true;
    } catch (e) {
      if (token !== loadSeq) return; // a newer load owns the list, and its errors
      if (append) throw e;
      // No network at all: what shows next is saved (or nothing), so the note must say so --
      // the supersede above already dropped whatever kept it up before.
      if (!isRejection(e)) markServingSaved(`${activitiesPrefix}saved`);
      const cached = isRejection(e) || filtered ? undefined : getCachedActivities(oid);
      items = cached?.items ?? [];
      total = cached?.total ?? 0;
    }
    if (token !== loadSeq) return; // a newer load started while this one was in flight
    // Below the token check: a superseded load must not leave the offline cache holding a page
    // the UI has already decided not to show -- e.g. the pre-filter page, after a filter change
    // resolved first. Only on success: the catch above produces an EMPTY list for a rejection,
    // and caching that would replace a good page with nothing, so the next offline load would
    // show an empty timeline instead of the last one the user actually saw.
    if (!append && fetched && !filtered) setCachedActivities(oid, { items, total });
    activities = mergeWindow(append, activities, pending, items);
    activityTotal = total + pending.length;
  }

  async function loadMore() {
    loadingMore = true;
    try { await loadActivities('append'); }
    catch (e) { error = (e as Error).message; }
    finally { loadingMore = false; }
  }

  $effect(() => { oid; loadObject(); });
  $effect(() => { oid; category; tagFilter; loadActivities('reset'); });
  $effect(() => { oid; if (tab === 'info') loadChildren(); });
  // A background replay can succeed while this view is mounted; without this the synthetic
  // pending entry it created keeps rendering next to the now-real row until the next remount.
  //
  // Only when the pass actually changed the queued creates this view renders, though: the
  // listeners fire after EVERY pass, empty queue included, and a flush runs on every
  // `visibilitychange`. Reloading unconditionally meant that switching away from the tab and
  // back re-fetched the timeline for no reason -- and, before `refresh` existed, threw away
  // every extra page the user had loaded.
  $effect(() => onOutboxFlushed(async (_resolved, changed) => {
    const rendered = activities.filter((a) => a.pending).map((a) => a.id);
    const queued = (await pendingActivities()).map((a) => a.id);
    if (!shouldReload(changed, rendered, queued)) return;
    loadObject();
    loadActivities('refresh');
  }));

  // The default tab is left out of the address, and the address is replaced only when it changes:
  // opening `/objects/5` must not turn into `/objects/5?tab=timeline` a moment later, which
  // breaks a back-button history entry's match and any test waiting for the plain URL.
  $effect(() => {
    const url = new URL(location.href);
    if (tab === 'timeline') url.searchParams.delete('tab'); else url.searchParams.set('tab', tab);
    // `?tag=` is read once, on load; the filter is session state from then on, like `category`,
    // so a stale tag left in the address would come back on a reload after being cleared.
    url.searchParams.delete('tag');
    const next = url.pathname + url.search;
    if (next !== location.pathname + location.search) history.replaceState(null, '', next);
  });

  function setTab(x: Tab) { tab = x; }
</script>

<main>
  {#if error}<p class="error">{error}</p>{/if}
  {#if object}
    <TopBar title={object.name} icon={typeIcon(object.type, $customTypes)} backTo="/">
      <button class="ghost" aria-label={$t('nav.edit')} onclick={() => go(`/objects/${oid}/edit`)}><Icon name="edit" /></button>
    </TopBar>

    {#if (object.ancestors ?? []).length > 0}
      <p class="breadcrumb">
        {#each object.ancestors ?? [] as a, i}
          <a href={`/objects/${a.id}`} onclick={(e) => { e.preventDefault(); go(`/objects/${a.id}`); }}>{a.name}</a>
          {#if i < (object.ancestors ?? []).length - 1} › {/if}
        {/each}
      </p>
    {/if}

    {#if object.cover_file_id}
      <img class="hero" src={fileUrl(object.cover_file_id)} alt="" />
    {/if}

    <div class="stats">
      <span class="stat"><b>{money(object.stats.total_cost_cents, $currency, $locale)}</b><span>{$t('object.total')}</span></span>
      <span class="stat"><b>{object.stats.activity_count}</b><span>{$t('object.activities')}</span></span>
      {#if object.counter_unit}
        <span class="stat"><b>{counter(object.stats.current_counter, object.counter_unit, $locale) || '—'}</b><span>{$t('object.current')}</span></span>
      {/if}
      {#if object.purchase_date}
        <span class="stat"><b>{fmtDate(object.purchase_date, $dateFormat)}</b><span>{$t('object.since')}</span></span>
      {/if}
    </div>

    <nav class="tabs">
      <button class:active={tab === 'timeline'} onclick={() => setTab('timeline')}>{$t('tab.timeline')}</button>
      <button class:active={tab === 'documents'} onclick={() => setTab('documents')}>{$t('tab.documents')}</button>
      <button class:active={tab === 'reminders'} onclick={() => setTab('reminders')}>{$t('tab.reminders')}{#if object.stats.due_reminder_count > 0}<span class="chip due">{object.stats.due_reminder_count}</span>{/if}</button>
      <button class:active={tab === 'info'} onclick={() => setTab('info')}>{$t('tab.info')}</button>
    </nav>

    {#if tab === 'timeline'}
      <Timeline
        objectId={oid} type={object.type} {activities} total={activityTotal} {loadingMore}
        onmore={loadMore} onlog={() => go(`/objects/${oid}/activities/new`)}
        unit={object.counter_unit} bind:category bind:tagFilter
      />
      <!-- The empty timeline puts this same action in the middle of the page, where the eye
           already is; two of them would be two calls to the same action. -->
      {#if activities.length > 0 || category !== '' || tagFilter !== null}
        <button class="primary fab" onclick={() => go(`/objects/${oid}/activities/new`)}>+ {$t('timeline.log')}</button>
      {/if}
    {:else if tab === 'documents'}
      <Documents objectId={oid} coverAttachmentId={object.cover_attachment_id} onchanged={loadObject} />
    {:else if tab === 'reminders'}
      <Reminders objectId={oid} unit={object.counter_unit} {activities} onchanged={() => { loadObject(); loadActivities('refresh'); }} />
    {:else}
      <h2>{object.name}</h2>
      <p class="muted">{typeLabel(object.type, $customTypes, $t, $typesLoaded)}</p>
      <TagChips tags={object.tags ?? []} />
      {#if object.description}<p class="desc">{object.description}</p>{/if}
      {#if object.purchase_price_cents !== null}<p class="muted">{$t('object.purchase-price')}: {money(object.purchase_price_cents, $currency, $locale)}</p>{/if}
      <h3>{$t('object.contents')}</h3>
      {#if children.length === 0}
        <p class="muted">{$t('object.contents-empty')}</p>
      {:else}
        <div class="list">
          {#each children as c (c.id)}<ObjectCard object={c} />{/each}
        </div>
      {/if}
      <button class="ghost" onclick={() => go(`/objects/new?parent_id=${oid}`)}>+ {$t('object.contents-add')}</button>
      <h3>{$t('insights.title')}</h3>
      <Insights objectId={oid} unit={object.counter_unit} hasContents={children.length > 0 || archivedChildCount > 0} />
      <div class="list info-actions">
        <button onclick={() => go(`/objects/${oid}/edit`)}>{$t('nav.edit')}</button>
        <a class="button-like" href={`/api/export?object_id=${oid}`}>{$t('object.export')}</a>
      </div>
    {/if}
  {:else if !error}
    <p class="muted">{$t('nav.loading')}</p>
  {/if}
</main>

<style>
  .hero { width: 100%; max-height: 240px; object-fit: cover; border-radius: var(--radius-md); }
  .breadcrumb { color: var(--muted); font-size: var(--text-sm); margin: var(--space-2) 0; }
  .breadcrumb a { color: inherit; }
  .desc { white-space: pre-wrap; margin: var(--space-2) 0; }
  .info-actions { margin-top: var(--space-4); }
  .tabs .chip { margin-left: var(--space-1); }
</style>
