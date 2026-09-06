<script lang="ts">
  import { untrack } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import Timeline from '../lib/Timeline.svelte';
  import Documents from '../lib/Documents.svelte';
  import Reminders from '../lib/Reminders.svelte';
  import Insights from '../lib/Insights.svelte';
  import { api, apiPage, fileUrl, isRejection, onOutboxFlushed, pendingOpsFor } from '../lib/api';
  import { getCachedActivities, getCachedObject, setCachedActivities, setCachedObject } from '../lib/object-cache';
  import { go } from '../lib/router';
  import { counter, fmtDate, money } from '../lib/format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { Activity, ActivityInput, Category, MemObject } from '../lib/types';
  import type { QueuedOp } from '../lib/outbox';

  let { id }: { id: string } = $props();
  const oid = $derived(Number(id));
  type Tab = 'timeline' | 'documents' | 'reminders' | 'info';
  let tab = $state<Tab>((new URLSearchParams(location.search).get('tab') as Tab) || 'timeline');
  let object = $state<MemObject | null>(null);
  let activities = $state<Activity[]>([]);
  /// How many activities match the current filter in total, page window aside.
  let activityTotal = $state(0);
  let loadingMore = $state(false);
  let category = $state<Category | ''>('');
  let error = $state('');
  const PAGE = 100;
  /// Must match MAX_LIMIT in src/api/activities.rs: the server clamps a bigger `limit` silently,
  /// so asking for more than this loses rows with no error to notice.
  const MAX_LIMIT = 500;

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
      pending: true,
    };
  }

  async function pendingActivities(): Promise<Activity[]> {
    const ops = await pendingOpsFor(`/objects/${oid}/activities`);
    return ops.filter((o) => !category || o.body.category === category).map(pendingToActivity);
  }

  /// Deliberately unfiltered by category, and deliberately including uploads: a queued upload
  /// landing changes an existing entry's thumbnail strip and the object's stats, which the
  /// pending-entry list alone says nothing about.
  async function outboxKey(): Promise<string> {
    const [creates, uploads] = await Promise.all([
      pendingOpsFor(`/objects/${oid}/activities`),
      pendingOpsFor(`/objects/${oid}/attachments`),
    ]);
    return [...creates, ...uploads].map((o) => o.id).join(',');
  }

  /// Which queued ops this view's rendering depends on, as a stable key: the creates it shows
  /// as pending entries, and the uploads that will change an existing entry's thumbnails. A
  /// flush pass only matters here if it changed that set -- see the `onOutboxFlushed`
  /// subscription.
  ///
  /// `null` means "not known": no load has committed yet, or one is in flight. A flush landing
  /// in that window must always reload -- comparing against a stale or not-yet-written key
  /// would skip the reload while the in-flight load goes on to commit a server page fetched
  /// BEFORE the replayed write landed, leaving that activity rendered neither as pending nor
  /// as real until the next remount.
  let pendingKey: string | null = null;
  /// Guards against two loads landing out of order: only the newest may commit its result.
  /// Several triggers can overlap (the `oid`/`category` effect, "load more", a flush), and
  /// whichever resolved last used to win regardless of which started last.
  let loadSeq = 0;

  /// - `reset` starts over from page one: what a filter or object change wants.
  /// - `append` fetches the next page and adds it to what is on screen.
  /// - `refresh` re-reads everything already on screen without shrinking it back to one page.
  ///
  /// A pending queued create for this object is prepended to any load that is not an append,
  /// since it will not appear in any page the server sends back until the outbox has replayed it.
  async function loadActivities(mode: 'reset' | 'append' | 'refresh' = 'reset') {
    const append = mode === 'append';
    const token = ++loadSeq;
    // The offset must count only rows the server itself sent back. A prepended pending entry
    // (see `pendingActivities` above) has no server-side page position at all -- counting it in
    // `activities.length` would shift every subsequent "load more" request back by one real
    // activity per pending op, silently skipping it.
    // `untrack`: this runs synchronously inside the `oid`/`category` $effect below, so reading
    // `activities` here made that effect depend on the very list it goes on to assign. The
    // effect then re-ran on its own result and started over from page one -- which is why
    // "Show N older" appeared to do nothing at all: the appended page was fetched, rendered,
    // and immediately replaced by a fresh page-one load.
    const loaded = untrack(() => activities.filter((a) => !a.pending).length);
    // Anything this load is about to replace is no longer what is on screen, so the key stops
    // describing it the moment the load starts. See the comment on `pendingKey`.
    if (!append) pendingKey = null;
    // A refresh must cover every page the user has already pulled in. Requesting one PAGE would
    // silently throw away every "load more" they did, which is what a flush -- fired on every
    // `visibilitychange`, so on merely switching away from the tab and back -- used to do. The
    // server clamps `limit` to MAX_LIMIT, so a window wider than that is fetched in chunks
    // rather than asked for in one request that would come back quietly truncated.
    const want = mode === 'refresh' ? Math.max(PAGE, loaded) : PAGE;
    const base = append ? loaded : 0;
    let items: Activity[] = [];
    let total = 0;
    try {
      while (items.length < want) {
        const params = new URLSearchParams({
          limit: String(Math.min(MAX_LIMIT, want - items.length)),
          offset: String(base + items.length),
        });
        if (category) params.set('category', category);
        const page = await apiPage<Activity>(`/objects/${oid}/activities?${params}`);
        items = [...items, ...page.items];
        total = page.total;
        if (page.items.length === 0) break; // the server has nothing more to give
      }
    } catch (e) {
      if (append) throw e;
      const cached = isRejection(e) ? undefined : getCachedActivities(oid);
      items = cached?.items ?? [];
      total = cached?.total ?? 0;
    }
    const pending = append ? [] : await pendingActivities();
    const key = append ? null : await outboxKey();
    if (token !== loadSeq) return; // a newer load started while this one was in flight
    // Below the token check: a superseded load must not leave the offline cache holding a page
    // the UI has already decided not to show -- e.g. the pre-filter page, after a filter change
    // resolved first -- since that cache is what the catch above serves when offline.
    if (!append) setCachedActivities(oid, { items, total });
    if (!append) pendingKey = key;
    activities = append ? [...activities, ...items] : [...pending, ...items];
    activityTotal = total + pending.length;
  }

  async function loadMore() {
    loadingMore = true;
    try { await loadActivities('append'); }
    catch (e) { error = (e as Error).message; }
    finally { loadingMore = false; }
  }

  $effect(() => { oid; loadObject(); });
  $effect(() => { oid; category; loadActivities('reset'); });
  // A background replay can succeed while this view is mounted; without this the synthetic
  // pending entry it created keeps rendering next to the now-real row until the next remount.
  //
  // Only when the pass actually changed the queued creates this view renders, though: the
  // listeners fire after EVERY pass, empty queue included, and a flush runs on every
  // `visibilitychange`. Reloading unconditionally meant that switching away from the tab and
  // back re-fetched the timeline for no reason -- and, before `refresh` existed, threw away
  // every extra page the user had loaded.
  $effect(() => onOutboxFlushed(async () => {
    const key = await outboxKey();
    if (pendingKey !== null && key === pendingKey) return;
    loadObject();
    loadActivities('refresh');
  }));
  $effect(() => {
    const url = new URL(location.href);
    url.searchParams.set('tab', tab);
    history.replaceState(null, '', url.pathname + url.search);
  });

  function setTab(x: Tab) { tab = x; }
</script>

<main>
  {#if error}<p class="error">{error}</p>{/if}
  {#if object}
    <TopBar title={object.name} backTo="/">
      <button class="ghost" aria-label={$t('nav.edit')} onclick={() => go(`/objects/${oid}/edit`)}>✎</button>
    </TopBar>

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
        <span class="stat"><b>{fmtDate(object.purchase_date, $locale)}</b><span>{$t('object.since')}</span></span>
      {/if}
    </div>

    <nav class="tabs">
      <button class:active={tab === 'timeline'} onclick={() => setTab('timeline')}>{$t('tab.timeline')}</button>
      <button class:active={tab === 'documents'} onclick={() => setTab('documents')}>{$t('tab.documents')}</button>
      <button class:active={tab === 'reminders'} onclick={() => setTab('reminders')}>{$t('tab.reminders')}{#if object.stats.due_reminder_count > 0}<span class="chip due">{object.stats.due_reminder_count}</span>{/if}</button>
      <button class:active={tab === 'info'} onclick={() => setTab('info')}>{$t('tab.info')}</button>
    </nav>

    {#if tab === 'timeline'}
      <Timeline objectId={oid} {activities} total={activityTotal} {loadingMore} onmore={loadMore} unit={object.counter_unit} bind:category />
      <button class="primary fab" onclick={() => go(`/objects/${oid}/activities/new`)}>+ {$t('timeline.log')}</button>
    {:else if tab === 'documents'}
      <Documents objectId={oid} coverAttachmentId={object.cover_attachment_id} onchanged={loadObject} />
    {:else if tab === 'reminders'}
      <Reminders objectId={oid} unit={object.counter_unit} {activities} onchanged={() => { loadObject(); loadActivities('refresh'); }} />
    {:else}
      <h2>{object.name}</h2>
      <p class="muted">{object.category}</p>
      {#if object.description}<p class="desc">{object.description}</p>{/if}
      {#if object.purchase_price_cents !== null}<p class="muted">{$t('object.purchase-price')}: {money(object.purchase_price_cents, $currency, $locale)}</p>{/if}
      <h3>{$t('insights.title')}</h3>
      <Insights objectId={oid} unit={object.counter_unit} />
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
  .hero { width: 100%; max-height: 240px; object-fit: cover; border-radius: var(--radius); }
  .desc { white-space: pre-wrap; margin: 8px 0; }
  .info-actions { margin-top: 16px; }
  .button-like { display: block; text-align: center; padding: 10px 16px; border-radius: var(--radius); background: var(--surface-2); color: var(--text); text-decoration: none; }
  .tabs .chip { margin-left: 4px; }
</style>
