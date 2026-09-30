<script lang="ts">
  import { errorMessage } from '../lib/api-error';
  import WeightHistory from '../lib/WeightHistory.svelte';
  import { tick, untrack } from 'svelte';
  import { MediaQuery } from 'svelte/reactivity';
  import TopBar from '../lib/TopBar.svelte';
  import Timeline from '../lib/Timeline.svelte';
  import Documents from '../lib/Documents.svelte';
  import Reminders from '../lib/Reminders.svelte';
  import ObjectSummary from '../lib/ObjectSummary.svelte';
  import ObjectDetails from '../lib/ObjectDetails.svelte';
  import Icon from '../lib/Icon.svelte';
  import LogAction from '../lib/LogAction.svelte';
  import { logOptions } from '../lib/log-options';
  import { energyLabelKey } from '../lib/energy';
  import { customTypes, typeIcon } from '../lib/type-registry';
  import { api, apiPage, isRejection, onOutboxFlushed, pendingOpsFor, markServingSaved, supersedeStale } from '../lib/api';
  import { dropCachedObject, getCachedActivities, getCachedObject, setCachedActivities, setCachedObject } from '../lib/object-cache';
  import { createSeq } from '../lib/seq-guard';
  import { persisted } from '../stores/persisted';
  import { insightsPath } from '../lib/insights';
  import { go } from '../lib/router';
  import { todayIso } from '../lib/format';
  import { t } from '../i18n';
  import * as Tabs from '$lib/components/ui/tabs/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import type { Activity, Category, EnergyOut, Insights as InsightsData, LastDone as LastDoneT, MemObject, Reminder, TripSummary } from '../lib/types';
  import { fetchWindow, mergeWindow, shouldReload, windowFor, type LoadMode } from '../lib/timeline-load';
  import { tagParam, offersTrip as offersTripFor, offersEnergy as offersEnergyFor, resourceCategory as resourceCategoryFor, pendingToActivity, filterPendingOps, nextUrl, resolvedObjectPath } from '../lib/object-detail';

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
  const TABS: readonly Tab[] = ['timeline', 'documents', 'reminders', 'info'];
  const queryTab = initialQuery.get('tab');
  let tab = $state<Tab>(initialTag !== null ? 'timeline' : TABS.find((x) => x === queryTab) ?? 'timeline');
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
  /** "+ Log" on this object: the same three actions the floating buttons used to be. */
  const logChoices = $derived(logOptions({
    oid,
    offersTrip,
    resourceCategory: offersEnergy ? resourceCategory : null,
    labels: { activity: $t('timeline.log'), trip: $t('trip.log'), resource: resourceLogLabel },
  }));
  let activities = $state<Activity[]>([]);
  /// How many activities match the current filter in total, page window aside.
  let activityTotal = $state(0);
  let loadingMore = $state(false);
  /** Whether this object's timeline has answered once (from the server or the cache), so the
   *  header "+ Log" hides only on a timeline known to be empty -- not while it is still loading. */
  let timelineLoaded = $state(false);
  let category = $state<Category | ''>('');
  /** Session-only, like `category`: a tag tapped on an entry narrows the timeline to it. */
  let tagFilter = $state<string | null>(initialTag);
  /** Session-only too: a "Last done" row tapped in the details narrows the timeline to its
   *  title (see `selectLastDone`). */
  let titleFilter = $state<string | null>(null);
  let children = $state<MemObject[]>([]);
  /** Archived children are not listed under Contents, but their costs still count with "Include
   *  contents", so having any is enough to offer the switch. */
  let archivedChildCount = $state(0);
  /** The "Last done" list, loaded only while the details are shown (Info tab, or the desktop
   *  pane) -- see `loadChildren`. */
  let lastDone = $state<LastDoneT[]>([]);
  /** The "Trips" totals, loaded only while the details are shown (Info tab, or the desktop
   *  pane), for the same reason -- see
   *  `loadTripSummary`. `null` until loaded, and again on a failed request. */
  let tripSummary = $state<TripSummary | null>(null);
  /** Distance and cost per charge, and when to charge next -- backs both the Info tab's Energy
   *  section and (via `energyData?.cost_per_counter_milli`) the Timeline's own trip-cost
   *  estimate and the Trips table's "Energy cost" row, so it is loaded proactively below rather
   *  than only while the Info tab is open, unlike `tripSummary`/`lastDone`. `null` until loaded,
   *  and again on a failed request, exactly like those two. */
  let energyData = $state<EnergyOut | null>(null);

  /** The Cost data: the summary's consumption figure, the Info tab's breakdown and (from
   *  1024 px) the pane's spend chart. Loaded once here rather than by each. `null` until loaded. */
  let insights = $state<InsightsData | null>(null);
  let insightsError = $state('');
  const insightsSeq = createSeq();
  /** Per device and not synced, like the Statistics screen's purchase switch: a way of looking, not data. */
  const includeContents = persisted('logb.insights.contents', false);
  /** Archived children are not listed under Contents, but their costs still count with "Include
   *  contents", so having any is enough to offer the switch. */
  const hasContents = $derived(children.length > 0 || archivedChildCount > 0);
  // An object without children always asks for its own figures, whatever the switch last said
  // on a house. Derived, so a `hasContents` flip that does not change the path does not refetch.
  const insightsUrl = $derived(insightsPath(oid, hasContents && $includeContents));

  async function loadInsights(path: string) {
    const token = insightsSeq.next();
    try {
      const d = await api<InsightsData>('GET', path);
      if (insightsSeq.current(token)) { insights = d; insightsError = ''; }
    } catch (e) {
      if (insightsSeq.current(token)) insightsError = errorMessage(e, $t);
    }
  }

  /** This object's due reminders, for the summary. Asked for only when the object says it has
   *  any, so an object with none costs no request. A failure just leaves the list empty: the
   *  Reminders tab is where they are managed, and its badge still counts them. */
  let dueReminders = $state<Reminder[]>([]);
  const dueSeq = createSeq();
  const dueCount = $derived(object?.stats.due_reminder_count ?? 0);
  async function loadDue() {
    const token = dueSeq.next();
    try {
      const rows = await api<Reminder[]>('GET', `/objects/${oid}/reminders`);
      if (dueSeq.current(token)) dueReminders = rows.filter((r) => r.due && r.done_at === null);
    } catch {
      if (dueSeq.current(token)) dueReminders = [];
    }
  }
  let error = $state('');
  /** The desktop left pane, scrolled back to the top on every navigation to another object. */
  let pane = $state<HTMLElement | null>(null);

  /** Whether the object's own reads (the Cost data, and the details' lists) may be asked for. Not
   *  for a temporary (negative) id: the server has no such object yet, and the outbox moves this
   *  page to the real id once it does. Nor before the object is known. A derived boolean, so a
   *  later reload of the object does not ask again. */
  const serverReady = $derived(oid > 0 && object !== null);
  function refreshInsights() { if (serverReady) loadInsights(insightsUrl); }

  /// Guards `loadObject` like `loadLastDone` below: a mount, an oid change and a flush can each
  /// start one, and the object just left must not land over the one navigated to.
  const objectSeq = createSeq();

  /** Only a genuine connectivity failure (see `isRejection`) may fall back to the cache — a
   *  401/403/404 is the server answering, and this object may simply belong to someone else. */
  async function loadObject() {
    const token = objectSeq.next();
    const target = oid;
    if (target < 0) {
      const pending = getCachedObject(target);
      if (pending) { object = pending; error = ''; return; }
    }
    try {
      const loaded = await api<MemObject>('GET', `/objects/${target}`);
      setCachedObject(target, loaded);
      if (!objectSeq.current(token)) return;
      object = loaded;
      error = '';
    } catch (e) {
      if (!objectSeq.current(token)) return;
      const cached = isRejection(e) ? undefined : getCachedObject(target);
      if (cached) object = cached;
      else error = errorMessage(e, $t);
    }
  }

  /// Guards `loadChildren` the same way; bumped on every navigation by the oid-reset effect.
  const childrenSeq = createSeq();

  /** The object's direct children, for the Contents section of the details. Loaded only while
   *  the details are shown (Info tab, or the desktop pane) -- the other tabs have no use for it. */
  async function loadChildren() {
    const token = childrenSeq.next();
    const target = oid;
    // Both at once: neither needs the other's answer.
    const [live, archived] = await Promise.allSettled([
      api<MemObject[]>('GET', `/objects?parent_id=${target}&archived=false`),
      api<MemObject[]>('GET', `/objects?parent_id=${target}&archived=true`),
    ]);
    if (!childrenSeq.current(token)) return;
    // Like Last done and Trips, a failed list is not worth a page error -- the details now load
    // on every desktop visit, and an offline one (object served from the cache) would otherwise
    // raise a banner over a page that is fine. Only the server answering no is said.
    if (live.status === 'fulfilled') children = live.value;
    else if (isRejection(live.reason)) error = errorMessage(live.reason, $t);
    archivedChildCount = archived.status === 'fulfilled' ? archived.value.length : 0;
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
    // The queue read runs while the page is fetched, not in front of it. Awaited below, where
    // its failure surfaces exactly as it did when it ran first; the no-op catch only keeps a
    // rejection from being reported as unhandled while the fetch is still in flight.
    const pendingRead = append ? Promise.resolve([]) : pendingActivities();
    pendingRead.catch(() => {});
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
    const pending = await pendingRead;
    if (token !== loadSeq) return; // a newer load started while this one was in flight
    // Below the token check: a superseded load must not leave the offline cache holding a page
    // the UI has already decided not to show -- e.g. the pre-filter page, after a filter change
    // resolved first. Only on success: the catch above produces an EMPTY list for a rejection,
    // and caching that would replace a good page with nothing, so the next offline load would
    // show an empty timeline instead of the last one the user actually saw.
    if (!append && fetched && !filtered) setCachedActivities(oid, { items, total });
    activities = mergeWindow(append, activities, pending, items);
    activityTotal = total + pending.length;
    timelineLoaded = true;
  }

  async function loadMore() {
    loadingMore = true;
    try { await loadActivities('append'); }
    catch (e) { error = errorMessage(e, $t); }
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
    children = [];
    archivedChildCount = 0;
    childrenSeq.invalidate();
    timelineLoaded = false;
    insights = null;
    insightsError = '';
    insightsSeq.invalidate();
    dueReminders = [];
    dueSeq.invalidate();
    // untrack: binding the pane must not re-run this reset.
    untrack(() => { if (pane) pane.scrollTop = 0; });
  });
  $effect(() => { oid; category; tagFilter; titleFilter; loadActivities('reset'); });
  // Two effects, each keyed on derived booleans only: `offersTrip` flipping true once the object
  // loads must not ask for the children and Last done a second time. `serverReady` (below) keeps
  // them off a temporary id and waits for the object, like the Cost data.
  $effect(() => { oid; if (detailsShown && serverReady) { loadChildren(); loadLastDone(); } });
  $effect(() => { oid; if (detailsShown && serverReady && offersTrip) loadTripSummary(); });
  // Unlike `loadTripSummary` above, not gated to the Info tab: the Timeline (the default tab)
  // needs `energyData.cost_per_counter_milli` for its own trip-cost estimate, so this loads as
  // soon as the object is known to have a fuel unit, whichever tab is open. `offersEnergy`
  // starts false (before `object` itself has loaded) and this effect re-runs once it flips true.
  $effect(() => { oid; if (offersEnergy) loadEnergy(); });
  // Figures for the other path (with or without contents) are dropped first, so a switch that
  // says "include contents" never sits next to figures that do not include them.
  $effect(() => { const path = insightsUrl; if (serverReady) { insights = null; insightsError = ''; loadInsights(path); } });
  // `dueCount` is derived, so a reload of the object that leaves the count alone does not ask again.
  // On the Reminders tab the tab's own list answers for the summary too (`onloaded` below): one
  // GET, not two. `untrack`: leaving the tab must not ask again for what it just loaded.
  $effect(() => {
    oid;
    if (dueCount === 0) { dueSeq.invalidate(); dueReminders = []; }
    else if (untrack(() => shownTab) !== 'reminders') loadDue();
  });
  // A background replay can succeed while this view is mounted; without this the synthetic
  // pending entry it created keeps rendering next to the now-real row until the next remount.
  //
  // Only when the pass actually changed the queued creates this view renders, though: the
  // listeners fire after EVERY pass, empty queue included, and a flush runs on every
  // `visibilitychange`. Reloading unconditionally meant that switching away from the tab and
  // back re-fetched the timeline for no reason -- and, before `refresh` existed, threw away
  // every extra page the user had loaded.
  $effect(() => onOutboxFlushed(async (resolved, changed) => {
    // An object created offline just reached the server: its temp id names nothing any more,
    // and the cached snapshot served under it would never pick up the real row. Move to the
    // real address in place (replace, so Back does not return to the dead temp page).
    const target = resolvedObjectPath(oid, resolved, location.search);
    if (target !== null) {
      dropCachedObject(oid);
      go(target, true);
      return;
    }
    const rendered = activities.filter((a) => a.pending).map((a) => a.id);
    const queued = (await pendingActivities()).map((a) => a.id);
    if (!shouldReload(changed, rendered, queued)) return;
    loadObject();
    loadActivities('refresh');
    refreshInsights();
    // A replayed offline charge or trip changes the Energy section's figures and the Trips
    // table the same way it changes the timeline above -- without this, either stayed stale
    // (the pending entry's own numbers, or none at all) until the next remount, exactly the
    // gap `loadActivities('refresh')` just above exists to close for the timeline itself. Same
    // guards as the effects that load them in the first place, since neither is worth loading
    // on an object that never offers it. The details refresh covers both while they are shown;
    // the Energy rate also feeds the Timeline, so it reloads regardless.
    if (detailsShown) refreshDetails();
    else if (offersEnergy) loadEnergy();
  }));

  // The default tab is left out of the address, and the address is replaced only when it changes:
  // opening `/objects/5` must not turn into `/objects/5?tab=timeline` a moment later, which
  // breaks a back-button history entry's match and any test waiting for the plain URL.
  $effect(() => {
    const next = nextUrl(location.href, tab);
    if (next !== location.pathname + location.search) history.replaceState(null, '', next);
  });

  function setTab(x: Tab) { tab = x; }
  /** A due reminder in the summary opens the Reminders tab; focus follows, so the keyboard user
   *  lands where the actions are (the tab list can render after the click, hence the tick). */
  async function openReminders() {
    setTab('reminders');
    await tick();
    document.querySelector<HTMLElement>('#object-sections [role="tab"][aria-selected="true"]')?.focus();
  }
  /** The wide layout keeps the details on screen, so after something changes them (a replayed
   *  offline entry, a CSV import, a reminder completed) they are asked for again. */
  function refreshDetails() {
    if (!detailsShown || !serverReady) return;
    loadLastDone();
    if (offersTrip) loadTripSummary();
    if (offersEnergy) loadEnergy();
  }
  /** The pane's skip link: to the selected tab, where the arrow keys take over. Not a hash
   *  navigation, which the router would not know about. */
  function skipToSections(e: MouseEvent) {
    e.preventDefault();
    document.querySelector<HTMLElement>('#object-sections [role="tab"][aria-selected="true"]')?.focus();
  }

  /** The one layout switch read in script (the plan's Decision 2): from 1024 px the details sit
   *  in the left pane and there is no Info tab. It is structural -- which container owns the
   *  details, whether a tab exists, when their data loads -- so CSS cannot make it alone. The
   *  query text is exactly Tailwind's `wide:` variant (app.tw.css). Svelte's MediaQuery reads
   *  matchMedia when this mounts, so the first paint is right, and follows resizes. Crossing
   *  1024 px moves the details to the other container, which remounts ObjectDetails: a CSV import
   *  in progress there is lost. Acceptable -- a window resized across it mid-import is rare. */
  const wide = new MediaQuery('(width >= 1024px)');
  /** An old `?tab=info` at desktop width shows the timeline: the details are already on screen. */
  const shownTab = $derived<Tab>(wide.current && tab === 'info' ? 'timeline' : tab);
  $effect(() => { if (wide.current && tab === 'info') tab = 'timeline'; });
  /** Whether the details are on screen: always from 1024 px, else while Info is open. */
  const detailsShown = $derived(wide.current || tab === 'info');
  /** The empty, unfiltered timeline carries its own log buttons in the middle of the page; a
   *  second "+ Log" beside them would be two calls to one action. */
  const timelineHasEntries = $derived(activities.length > 0 || category !== '' || tagFilter !== null || titleFilter !== null);
  /** The header "+ Log" stays while the timeline loads and goes only once it is known to be
   *  empty and unfiltered, so it does not pop in after the entries arrive. */
  const headerLog = $derived(shownTab !== 'timeline' || !timelineLoaded || timelineHasEntries);
  /** The Timeline draws its empty state (and its log buttons) before the first load answers;
   *  while the header "+ Log" is up, the empty state leaves its buttons out, so one "+ Log" shows
   *  at a time. */
  const emptyStateLog = $derived(!(wide.current && headerLog));
</script>

<main>
  {#if error}<p data-testid="page-error" class="m-0 mb-3 text-sm font-medium text-destructive">{error}</p>{/if}
  {#if object}
    <TopBar title={object.name} icon={typeIcon(object.type, $customTypes)} backTo="/">
      {#if wide.current && headerLog}
        <LogAction options={logChoices} onpick={(o) => go(o.path)} placement="header" />
      {/if}
      <!-- Icon only on a phone, icon and word from 1024 px; the name is "Edit" either way. -->
      <Button variant="outline" class="min-h-11 min-w-11 gap-1.5 px-3" aria-label={$t('nav.edit')} onclick={() => go(`/objects/${oid}/edit`)}>
        <Icon name="edit" size={18} /><span class="max-wide:hidden">{$t('nav.edit')}</span>
      </Button>
    </TopBar>

    {#if (object.ancestors ?? []).length > 0}
      <nav aria-label={$t('object.breadcrumb')} class="mb-2 text-sm text-muted-foreground">
        {#each object.ancestors ?? [] as a, i (a.id)}
          <a href={`/objects/${a.id}`}
             class="text-muted-foreground no-underline underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring"
             onclick={(e) => { e.preventDefault(); go(`/objects/${a.id}`); }}>{a.name}</a>{#if i < (object.ancestors ?? []).length - 1}<span aria-hidden="true"> › </span>{/if}
        {/each}
      </nav>
    {/if}

    <!-- One column on a phone, two panes from 1024 px. The summary is the same markup in both;
         the details are rendered once, in the pane or under Info (see `wide`). The pane is
         sticky and scrolls on its own: it is taller than the screen, and a sticky box taller
         than the screen would hide its end until the timeline ran out. `-mx-1 px-1` leaves
         room for focus rings, which a scroller clips. `*:shrink-0`: a column flex box with a
         max-height shrinks its children to fit instead of scrolling, which squashed the cover. -->
    <div class="flex flex-col gap-4 wide:grid wide:grid-cols-[300px_minmax(0,1fr)] wide:items-start wide:gap-6">
      <section aria-label={$t('object.summary')} bind:this={pane}
               class="flex flex-col gap-4 *:shrink-0 wide:sticky wide:top-20 wide:-mx-1 wide:max-h-[calc(100dvh-6rem)] wide:overflow-y-auto wide:overscroll-contain wide:px-1 wide:pb-4 wide:[scrollbar-width:thin]
                      wide:after:pointer-events-none wide:after:sticky wide:after:bottom-0 wide:after:-mt-4 wide:after:block wide:after:h-4 wide:after:shrink-0 wide:after:bg-linear-to-t wide:after:from-background wide:after:to-transparent wide:after:content-['']">
        {#if wide.current}
          <!-- The pane is long; a keyboard user can jump past it to the tabs. Hidden until focused. -->
          <a href="#object-sections" onclick={skipToSections}
             data-slot="skip-link"
             class="sr-only no-underline focus-visible:not-sr-only focus-visible:self-start focus-visible:rounded-md focus-visible:bg-card focus-visible:px-3 focus-visible:py-2 focus-visible:text-sm focus-visible:font-medium focus-visible:text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring">{$t('object.skip-to-tabs')}</a>
        {/if}
        <ObjectSummary {object} {insights} due={dueReminders} onreminders={openReminders} />
        {#if wide.current}{@render details()}{/if}
      </section>

      <div id="object-sections" class="flex min-w-0 flex-col gap-4">
        {#if object.type === 'body'}
          <WeightHistory objectId={oid} unit={object.weight_unit ?? 'kg'} />
        {/if}
        <Tabs.Root value={shownTab} onValueChange={(v) => setTab(v as Tab)}>
          <Tabs.List aria-label={$t('object.sections')}>
            <Tabs.Trigger value="timeline">{$t('tab.timeline')}</Tabs.Trigger>
            <Tabs.Trigger value="documents">{$t('tab.documents')}</Tabs.Trigger>
            <Tabs.Trigger value="reminders">
              {$t('tab.reminders')}
              {#if object.stats.due_reminder_count > 0}
                <!-- The number is for the eye; the words after it are for everyone else. -->
                <span data-testid="tab-due-badge" aria-hidden="true"
                      class="inline-grid h-5 min-w-5 place-items-center rounded-full bg-destructive px-1 text-xs font-semibold text-destructive-foreground tabular-nums">{object.stats.due_reminder_count}</span>
                <span class="sr-only">{$t('tab.reminders-due', { n: object.stats.due_reminder_count })}</span>
              {/if}
            </Tabs.Trigger>
            {#if !wide.current}<Tabs.Trigger value="info">{$t('tab.info')}</Tabs.Trigger>{/if}
          </Tabs.List>

          <Tabs.Content value="timeline">
            {#if shownTab === 'timeline'}
              <Timeline
                objectId={oid} type={object.type} weightUnit={object.weight_unit} {activities} total={activityTotal} {loadingMore} loaded={timelineLoaded}
                onmore={loadMore} onlog={emptyStateLog ? () => go(`/objects/${oid}/activities/new`) : undefined}
                ontriplog={emptyStateLog && offersTrip ? () => go(`/objects/${oid}/activities/new?category=trip`) : undefined}
                onchargelog={emptyStateLog && offersEnergy ? () => go(`/objects/${oid}/activities/new?category=${resourceCategory}`) : undefined}
                unit={object.counter_unit} fuelUnit={object.resource_unit ?? object.fuel_unit} resourceKind={object.resource_kind} energyRate={energyData?.cost_per_counter_milli ?? null}
                bind:category bind:tagFilter bind:titleFilter
              />
            {/if}
          </Tabs.Content>
          <Tabs.Content value="documents">
            {#if shownTab === 'documents'}<Documents objectId={oid} coverAttachmentId={object.cover_attachment_id} onchanged={loadObject} />{/if}
          </Tabs.Content>
          <Tabs.Content value="reminders">
            {#if shownTab === 'reminders'}
              <!-- Completing a reminder can log an entry with a cost, so the Cost data follows. -->
              <Reminders body={object.type === 'body'} objectId={oid} unit={object.counter_unit} {activities}
                         onloaded={(rows) => { dueSeq.invalidate(); dueReminders = rows.filter((r) => r.due && r.done_at === null); }}
                         onchanged={() => { loadObject(); loadActivities('refresh'); refreshInsights(); refreshDetails(); }} />
            {/if}
          </Tabs.Content>
          {#if !wide.current}
            <Tabs.Content value="info">{#if shownTab === 'info'}{@render details()}{/if}</Tabs.Content>
          {/if}
        </Tabs.Root>
      </div>
    </div>

    <!-- Below 1024 px "+ Log" floats over the timeline; from 1024 px it is in the header above. -->
    {#if !wide.current && shownTab === 'timeline' && timelineHasEntries}
      <div class="fab-pos flex gap-2">
        <LogAction options={logChoices} onpick={(o) => go(o.path)} />
      </div>
    {/if}
  {:else if !error}
    <p class="m-0 text-sm text-muted-foreground">{$t('nav.loading')}</p>
  {/if}
</main>

{#snippet details()}
  {#if object}
    <ObjectDetails
      {object} {insights} {insightsError} {lastDone} {tripSummary} energy={energyData} inside={children}
      {hasContents} contents={$includeContents} {offersTrip} {offersEnergy}
      oncontents={(on) => includeContents.set(on)} onlastdone={selectLastDone} onimported={() => { loadActivities('refresh'); refreshInsights(); refreshDetails(); }}
    />
  {/if}
{/snippet}
