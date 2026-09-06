<script module lang="ts">
  import type { Activity, MemObject } from '../lib/types';

  /** The last object and activities page fetched for each id, kept only for the lifetime of
   *  this tab (module scope, so it survives navigating away and back — a fresh component
   *  instance would otherwise lose it on every remount). A dead connection can still show the
   *  object it showed a moment ago, and a queued create on top of that (see `pendingActivities`
   *  below) is what makes an offline log visible immediately instead of behind a "failed to
   *  fetch" screen. */
  const objectCache = new Map<number, MemObject>();
  const activityCache = new Map<number, { items: Activity[]; total: number }>();
</script>

<script lang="ts">
  import TopBar from '../lib/TopBar.svelte';
  import Timeline from '../lib/Timeline.svelte';
  import Documents from '../lib/Documents.svelte';
  import Reminders from '../lib/Reminders.svelte';
  import Insights from '../lib/Insights.svelte';
  import { api, apiPage, fileUrl, pendingOpsFor } from '../lib/api';
  import { go } from '../lib/router';
  import { counter, fmtDate, money } from '../lib/format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { ActivityInput, Category } from '../lib/types';
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

  async function loadObject() {
    try {
      object = await api<MemObject>('GET', `/objects/${oid}`);
      objectCache.set(oid, object);
      error = '';
    } catch (e) {
      const cached = objectCache.get(oid);
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
   *  until the phone gets signal back, which is exactly the failure this task exists to avoid. */
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
    };
  }

  async function pendingActivities(): Promise<Activity[]> {
    const ops = await pendingOpsFor(`/objects/${oid}/activities`);
    return ops.filter((o) => !category || o.body.category === category).map(pendingToActivity);
  }

  /// `append` fetches the next page and adds to what is on screen; otherwise it starts over,
  /// which is what a filter change wants. A pending queued create for this object is prepended
  /// to a fresh (non-append) load, since it will not appear in any page the server sends back
  /// until the outbox has replayed it.
  async function loadActivities(append = false) {
    const params = new URLSearchParams({ limit: String(PAGE), offset: String(append ? activities.length : 0) });
    if (category) params.set('category', category);
    let items: Activity[] = [];
    let total = 0;
    try {
      const page = await apiPage<Activity>(`/objects/${oid}/activities?${params}`);
      items = page.items;
      total = page.total;
      if (!append) activityCache.set(oid, { items, total });
    } catch (e) {
      if (append) throw e;
      const cached = activityCache.get(oid);
      items = cached?.items ?? [];
      total = cached?.total ?? 0;
    }
    const pending = append ? [] : await pendingActivities();
    activities = append ? [...activities, ...items] : [...pending, ...items];
    activityTotal = total + pending.length;
  }

  async function loadMore() {
    loadingMore = true;
    try { await loadActivities(true); }
    catch (e) { error = (e as Error).message; }
    finally { loadingMore = false; }
  }

  $effect(() => { oid; loadObject(); });
  $effect(() => { oid; category; loadActivities(); });
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
      <Reminders objectId={oid} unit={object.counter_unit} {activities} onchanged={() => { loadObject(); loadActivities(); }} />
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
