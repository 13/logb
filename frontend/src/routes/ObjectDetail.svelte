<script lang="ts">
  import WeightHistory from '../lib/WeightHistory.svelte';
  import { untrack } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import Timeline from '../lib/Timeline.svelte';
  import Documents from '../lib/Documents.svelte';
  import Reminders from '../lib/Reminders.svelte';
  import Insights from '../lib/Insights.svelte';
  import Icon from '../lib/Icon.svelte';
  import ObjectCard from '../lib/ObjectCard.svelte';
  import TagChips from '../lib/TagChips.svelte';
  import LastDone from '../lib/LastDone.svelte';
  import TripTotals from '../lib/TripTotals.svelte';
  import EnergyFigures from '../lib/EnergyFigures.svelte';
  import ResourceCsvImport from '../lib/ResourceCsvImport.svelte';
  import { energyLabelKey } from '../lib/energy';
  import { customTypes, typeIcon, typeLabel, typesLoaded } from '../lib/type-registry';
  import { api, apiPage, fileUrl, isRejection, onOutboxFlushed, pendingOpsFor, markServingSaved, supersedeStale } from '../lib/api';
  import { getCachedActivities, getCachedObject, setCachedActivities, setCachedObject } from '../lib/object-cache';
  import { go } from '../lib/router';
  import { counter, fmtDate, money, todayIso } from '../lib/format';
  import { dateFormat } from '../stores/date-format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { Activity, Category, EnergyOut, LastDone as LastDoneT, MemObject, TripSummary } from '../lib/types';
  import { fetchWindow, mergeWindow, shouldReload, windowFor, type LoadMode } from '../lib/timeline-load';
  import { tagParam, offersTrip as offersTripFor, offersEnergy as offersEnergyFor, resourceCategory as resourceCategoryFor, pendingToActivity, filterPendingOps, nextUrl } from '../lib/object-detail';

  let { id }: { id: string } = $props();
  const oid = $derived(Number(id));
  type Tab = 'timeline' | 'documents' | 'reminders' | 'info';
  const initialQuery = new URLSearchParams(location.search);
  /** A `?tag=` link (a chip tapped in search) opens the timeline already narrowed to that tag.
   *  `tagParam` (the `?tag=` query parameter of the CURRENT address, trimmed; blank/absent is
   *  `null`) is read again, not just once here: `App.svelte`'s route table maps every
   *  `/objects/:id` to this same `ObjectDetail` instance, so navigating from one object to
   *  another (a card tapped under Contents, say) only changes the `id` prop -- it does not
   *  remount this component or re-run the `let` initialisers below. The oid-reset effect
   *  further down calls it again on every such navigation, so a fresh `?tag=` on the object
   *  just navigated to still wins, the same way it does here at mount. */
  const initialTag = tagParam(location.search);
  let tab = $state<Tab>(initialTag !== null ? 'timeline' : (initialQuery.get('tab') as Tab) || 'timeline');
  let object = $state<MemObject | null>(null);
  /** Whether a trip can be logged here at all -- the same condition `categoriesFor`'s own
   *  `counterUnit` argument checks, so "+ Log trip" (both the FAB and the empty-state one) and
   *  the category the entry form actually offers never disagree. */
  const offersTrip = $derived(offersTripFor(object));
  /** Whether a charge (or fill) can be logged here at all -- "+ Log charge" (both the FAB and
   *  the Timeline empty state), and whether the Energy section's own figures are worth loading. */
  const offersEnergy = $derived(offersEnergyFor(object));
  const resourceCategory = $derived(resourceCategoryFor(object));
  const resourceLogLabel = $derived(object?.resource_kind === 'water' ? $t('water.log') : $t(`${energyLabelKey(object?.resource_unit ?? object?.fuel_unit ?? null)}-log`));
  let activities = $state<Activity[]>([]);
  /// How many activities match the current filter in total, page window aside.
  let activityTotal = $state(0);
  let loadingMore = $state(false);
  let category = $state<Category | ''>('');
  /** Session-only, like `category`: a tag tapped on an entry narrows the timeline to it. */
  let tagFilter = $state<string | null>(initialTag);
  /** Session-only too: a "Last done" row tapped on the Info tab narrows the timeline to its
   *  title (see `selectLastDone`). */
  let titleFilter = $state<string | null>(null);
  let children = $state<MemObject[]>([]);
  /** Archived children are not listed under Contents, but their costs still count with "Include
   *  contents", so having any is enough to offer the switch. */
  let archivedChildCount = $state(0);
  /** The Info tab's "Last done" list, loaded only while that tab is open -- see `loadChildren`. */
  let lastDone = $state<LastDoneT[]>([]);
  /** The Info tab's "Trips" totals, loaded the same way and for the same reason -- see
   *  `loadTripSummary`. `null` until loaded, and again on a failed request. */
  let tripSummary = $state<TripSummary | null>(null);
  /** Distance and cost per charge, and when to charge next -- backs both the Info tab's Energy
   *  section and (via `energyData?.cost_per_counter_milli`) the Timeline's own trip-cost
   *  estimate and the Trips table's "Energy cost" row, so it is loaded proactively below rather
   *  than only while the Info tab is open, unlike `tripSummary`/`lastDone`. `null` until loaded,
   *  and again on a failed request, exactly like those two. */
  let energyData = $state<EnergyOut | null>(null);
  let error = $state('');

  /** Only a genuine connectivity failure (see `isRejection`) may fall back to the cache — a
   *  401/403/404 is the server answering, and this object may simply belong to someone else. */
  async function loadObject() {
    if (oid < 0) {
      const pending = getCachedObject(oid);
      if (pending) { object = pending; error = ''; return; }
    }
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

  /// Guards a `loadLastDone` response against a since-superseded request, the same way
  /// `loadSeq` guards the timeline -- see the oid-reset effect below, which bumps this on every
  /// navigation to another object so a response for the object just left cannot land (or be
  /// tapped into) after this component has moved on to a different one.
  let lastDoneSeq = 0;

  /** The "Last done" list, for the Info tab. Hidden by `LastDone.svelte` itself when empty, so
   *  a failed request (like an offline load) just leaves it hidden rather than showing an error
   *  of its own -- this list is a convenience shortcut into the timeline, not primary data. */
  async function loadLastDone() {
    const token = ++lastDoneSeq;
    try {
      const rows = await api<LastDoneT[]>('GET', `/objects/${oid}/last-done`);
      if (token === lastDoneSeq) lastDone = rows;
    } catch {
      if (token === lastDoneSeq) lastDone = [];
    }
  }

  /// Guards `loadTripSummary` against a since-superseded request, the same way `lastDoneSeq`
  /// guards "Last done" -- see the oid-reset effect below.
  let tripSummarySeq = 0;

  /** The Info tab's "Trips" totals. Hidden by `TripTotals.svelte` itself when there are none, so
   *  a failed request (like an offline load) just leaves it hidden rather than showing an error
   *  of its own -- like "Last done", this is a convenience summary, not primary data. */
  async function loadTripSummary() {
    const token = ++tripSummarySeq;
    try {
      const s = await api<TripSummary>('GET', `/objects/${oid}/trips/summary?today=${todayIso()}`);
      if (token === tripSummarySeq) tripSummary = s;
    } catch {
      if (token === tripSummarySeq) tripSummary = null;
    }
  }

  /// Guards `loadEnergy` against a since-superseded request, the same way `tripSummarySeq` does.
  let energySeq = 0;

  /** The Energy section's figures (and the Timeline/Trips-table rate riding along with them).
   *  Hidden by `EnergyFigures.svelte` itself when every figure is null, so a failed request just
   *  leaves it hidden -- same reasoning as `loadTripSummary`. */
  async function loadEnergy() {
    const token = ++energySeq;
    try {
      const e = await api<EnergyOut>('GET', `/objects/${oid}/energy`);
      if (token === energySeq) energyData = e;
    } catch {
      if (token === energySeq) energyData = null;
    }
  }

  /** A "Last done" row switches to the timeline, narrowed to its title. */
  function selectLastDone(title: string) {
    titleFilter = title;
    // A category or tag chosen earlier would hide the very entries the row stands for.
    category = '';
    tagFilter = null;
    tab = 'timeline';
  }

  async function pendingActivities(): Promise<Activity[]> {
    const ops = await pendingOpsFor(`/objects/${oid}/activities`);
    return filterPendingOps(ops, { category, tagFilter, titleFilter }).map((o) => pendingToActivity(o, oid));
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
    const filtered = category !== '' || tagFilter !== null || titleFilter !== null;
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
        if (titleFilter) params.set('title', titleFilter);
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
  // The instance is reused across objects (see `tagParam`'s comment above): without this, a
  // category/tag/title filter chosen while looking at one object would silently keep narrowing
  // the next one's timeline, and a stale "Last done" row from the object just left could still
  // be shown -- and tapped into -- after this component has moved on to a different one. Placed
  // before the two effects below, in the same flush order they run in on an oid change, so both
  // already see the reset values instead of loading once with the old ones and once more right
  // after with the new.
  $effect(() => {
    oid;
    category = '';
    tagFilter = tagParam(location.search);
    titleFilter = null;
    lastDone = [];
    lastDoneSeq++;
    tripSummary = null;
    tripSummarySeq++;
    energyData = null;
    energySeq++;
  });
  $effect(() => { oid; category; tagFilter; titleFilter; loadActivities('reset'); });
  $effect(() => { oid; if (tab === 'info') { loadChildren(); loadLastDone(); if (offersTrip) loadTripSummary(); } });
  // Unlike `loadTripSummary` above, not gated to the Info tab: the Timeline (the default tab)
  // needs `energyData.cost_per_counter_milli` for its own trip-cost estimate, so this loads as
  // soon as the object is known to have a fuel unit, whichever tab is open. `offersEnergy`
  // starts false (before `object` itself has loaded) and this effect re-runs once it flips true.
  $effect(() => { oid; if (offersEnergy) loadEnergy(); });
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
    // A replayed offline charge or trip changes the Energy section's figures and the Trips
    // table the same way it changes the timeline above -- without this, either stayed stale
    // (the pending entry's own numbers, or none at all) until the next remount, exactly the
    // gap `loadActivities('refresh')` just above exists to close for the timeline itself. Same
    // guards as the effects that load them in the first place, since neither is worth loading
    // on an object that never offers it.
    if (offersEnergy) loadEnergy();
    if (offersTrip) loadTripSummary();
  }));

  // The default tab is left out of the address, and the address is replaced only when it changes:
  // opening `/objects/5` must not turn into `/objects/5?tab=timeline` a moment later, which
  // breaks a back-button history entry's match and any test waiting for the plain URL.
  $effect(() => {
    const next = nextUrl(location.href, tab);
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
      {#if object.type !== 'body'}<span class="stat"><b>{money(object.stats.total_cost_cents, $currency, $locale)}</b><span>{$t('object.total')}</span></span>{/if}
      <span class="stat"><b>{object.stats.activity_count}</b><span>{$t('object.activities')}</span></span>
      {#if object.counter_unit}
        <span class="stat"><b>{counter(object.stats.current_counter, object.counter_unit, $locale) || '—'}</b><span>{$t('object.current')}</span></span>
      {/if}
      {#if object.purchase_date}
        <span class="stat"><b>{fmtDate(object.purchase_date, $dateFormat)}</b><span>{$t('object.since')}</span></span>
      {/if}
    </div>

    {#if object.type === 'body'}
      <WeightHistory objectId={oid} unit={object.weight_unit ?? 'kg'} />
    {/if}

    <nav class="tabs">
      <button class:active={tab === 'timeline'} onclick={() => setTab('timeline')}>{$t('tab.timeline')}</button>
      <button class:active={tab === 'documents'} onclick={() => setTab('documents')}>{$t('tab.documents')}</button>
      <button class:active={tab === 'reminders'} onclick={() => setTab('reminders')}>{$t('tab.reminders')}{#if object.stats.due_reminder_count > 0}<span class="chip due">{object.stats.due_reminder_count}</span>{/if}</button>
      <button class:active={tab === 'info'} onclick={() => setTab('info')}>{$t('tab.info')}</button>
    </nav>

    {#if tab === 'timeline'}
      <Timeline
        objectId={oid} type={object.type} weightUnit={object.weight_unit} {activities} total={activityTotal} {loadingMore}
        onmore={loadMore} onlog={() => go(`/objects/${oid}/activities/new`)}
        ontriplog={offersTrip ? () => go(`/objects/${oid}/activities/new?category=trip`) : undefined}
        onchargelog={offersEnergy ? () => go(`/objects/${oid}/activities/new?category=${resourceCategory}`) : undefined}
        unit={object.counter_unit} fuelUnit={object.resource_unit ?? object.fuel_unit} resourceKind={object.resource_kind} energyRate={energyData?.cost_per_counter_milli ?? null}
        bind:category bind:tagFilter bind:titleFilter
      />
      <!-- The empty timeline puts this same action in the middle of the page, where the eye
           already is; two of them would be two calls to the same action. -->
      {#if activities.length > 0 || category !== '' || tagFilter !== null || titleFilter !== null}
        <div class="fab-row">
          {#if offersTrip}
            <!-- Not `.ghost`: this floats over the scrolling timeline, and a transparent
                 button there shows whatever card/text is currently scrolled beneath it through
                 its own label. `.fab-secondary` gives it the same solid surface + border a card
                 has, so it reads as a button regardless of what is behind it. -->
            <button class="fab-btn fab-secondary" onclick={() => go(`/objects/${oid}/activities/new?category=trip`)}>+ {$t('trip.log')}</button>
          {/if}
          {#if offersEnergy}
            <button class="fab-btn fab-secondary" onclick={() => go(`/objects/${oid}/activities/new?category=${resourceCategory}`)}>+ {resourceLogLabel}</button>
          {/if}
          <button class="primary fab-btn" onclick={() => go(`/objects/${oid}/activities/new`)}>+ {$t('timeline.log')}</button>
        </div>
      {/if}
    {:else if tab === 'documents'}
      <Documents objectId={oid} coverAttachmentId={object.cover_attachment_id} onchanged={loadObject} />
    {:else if tab === 'reminders'}
      <Reminders body={object.type === 'body'} objectId={oid} unit={object.counter_unit} {activities} onchanged={() => { loadObject(); loadActivities('refresh'); }} />
    {:else}
      <h2>{object.name}</h2>
      <p class="muted">{typeLabel(object.type, $customTypes, $t, $typesLoaded)}</p>
      <TagChips tags={object.tags ?? []} />
      {#if object.description}<p class="desc">{object.description}</p>{/if}
      {#if object.purchase_price_cents !== null}<p class="muted">{$t('object.purchase-price')}: {money(object.purchase_price_cents, $currency, $locale)}</p>{/if}
      <LastDone items={lastDone} {object} onselect={selectLastDone} />
      {#if offersTrip}
        <TripTotals summary={tripSummary} unit={object.counter_unit as 'km' | 'mi'} energyRate={energyData?.cost_per_counter_milli ?? null} />
      {/if}
      {#if offersEnergy}<EnergyFigures energy={energyData} unit={object.counter_unit} />{/if}
      {#if object.resource_kind}<ResourceCsvImport objectId={oid} mode={object.measurement_mode} onimported={() => loadActivities('refresh')} />{/if}
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
  /* A second FAB ("+ Log trip") beside the usual one, only on a km/mi object. `.fab` itself
     (app.css) is `position: fixed`, sized for exactly one button -- two of those stacked on top
     of each other would overlap, not sit side by side. This wrapper takes over the fixed
     positioning (mirroring `.fab`'s own rules, including its two responsive overrides below) and
     lays its buttons out with `.fab-btn`, `.fab`'s own look with no position of its own. */
  .fab-row {
    position: fixed; right: var(--space-4); bottom: calc(var(--space-4) + env(safe-area-inset-bottom));
    z-index: 6; display: flex; gap: var(--space-2);
  }
  .fab-btn { border-radius: var(--radius-full); padding: var(--space-3) var(--space-4); box-shadow: 0 4px 12px rgba(0,0,0,.25); }
  /* A solid surface + border, not `.ghost`'s transparent: see the comment on the button itself. */
  .fab-secondary { background: var(--surface); border: 1px solid var(--border); color: var(--text); }
  @media (width < 900px) {
    .fab-row { bottom: calc(var(--space-4) + var(--navbar) + env(safe-area-inset-bottom)); }
  }
  @media (width >= 900px) {
    .fab-row { right: max(var(--space-5), calc((100vw - 240px - 1100px) / 2 + var(--space-5))); }
  }
</style>
