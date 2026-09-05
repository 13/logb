<script lang="ts">
  import TopBar from '../lib/TopBar.svelte';
  import Timeline from '../lib/Timeline.svelte';
  import Documents from '../lib/Documents.svelte';
  import Reminders from '../lib/Reminders.svelte';
  import { api, fileUrl } from '../lib/api';
  import { go } from '../lib/router';
  import { counter, fmtDate, money } from '../lib/format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { Activity, Category, MemObject } from '../lib/types';

  let { id }: { id: string } = $props();
  const oid = $derived(Number(id));
  type Tab = 'timeline' | 'documents' | 'reminders' | 'info';
  let tab = $state<Tab>((new URLSearchParams(location.search).get('tab') as Tab) || 'timeline');
  let object = $state<MemObject | null>(null);
  let activities = $state<Activity[]>([]);
  let category = $state<Category | ''>('');
  let error = $state('');

  async function loadObject() {
    try { object = await api<MemObject>('GET', `/objects/${oid}`); }
    catch (e) { error = (e as Error).message; }
  }
  async function loadActivities() {
    const q = category ? `?category=${category}` : '';
    activities = await api<Activity[]>('GET', `/objects/${oid}/activities${q}`);
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
      <Timeline objectId={oid} {activities} unit={object.counter_unit} bind:category />
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
