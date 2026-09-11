<script lang="ts">
  import TopBar from '../lib/TopBar.svelte';
  import ObjectCard from '../lib/ObjectCard.svelte';
  import { api } from '../lib/api';
  import { go } from '../lib/router';
  import { t } from '../i18n';
  import type { MemObject, Reminder } from '../lib/types';
  import Icon from '../lib/Icon.svelte';

  let objects = $state<MemObject[]>([]);
  let due = $state<Reminder[]>([]);
  let soon = $state<Reminder[]>([]);
  let archived = $state(false);
  let loading = $state(true);
  let error = $state('');

  async function load() {
    loading = true; error = '';
    try {
      objects = await api<MemObject[]>('GET', `/objects?archived=${archived}`);
      const all = await api<Reminder[]>('GET', '/reminders/due?within_days=30');
      due = all.filter((r) => r.due);
      soon = all.filter((r) => !r.due);
    } catch (e) { error = (e as Error).message; } finally { loading = false; }
  }
  // one load on mount and on every toggle of `archived`
  $effect(() => { archived; load(); });

  async function snooze(r: Reminder) {
    try { await api('POST', `/reminders/${r.id}/snooze`, { days: 7 }); await load(); }
    catch (e) { error = (e as Error).message; }
  }
</script>

<main>
  <TopBar title={$t('dash.title')} showSettings>
    <button class="ghost" aria-label={$t('search.title')} onclick={() => go('/search')}><Icon name="search" /></button>
  </TopBar>

  {#if due.length > 0}
    <div class="banner">
      <b>{due.length === 1 ? $t('dash.due-one') : $t('dash.due', { n: due.length })}</b>
      <ul>
        {#each due.slice(0, 5) as r (r.id)}
          <li>
            <a href={`/objects/${r.object_id}`} onclick={(e) => { e.preventDefault(); go(`/objects/${r.object_id}?tab=reminders`); }}>{r.object_name}: {r.title}</a>
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
            </span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#if error}<p class="error">{error}</p>{/if}
  {#if loading}
    <p class="muted">{$t('nav.loading')}</p>
  {:else if objects.length === 0}
    <p class="muted">{$t('dash.empty')}</p>
  {:else}
    <div class="list">
      {#each objects as o (o.id)}<ObjectCard object={o} />{/each}
    </div>
  {/if}

  <label class="row toggle"><input type="checkbox" bind:checked={archived} /> {$t('dash.show-archived')}</label>
  <button class="primary fab" onclick={() => go('/objects/new')}>+ {$t('dash.new')}</button>
</main>

<style>
  .toggle { margin-top: 18px; color: var(--muted); font-size: .9rem; }
  .toggle input { flex: none; width: 20px; height: 20px; }
  .banner ul { margin: 6px 0 0 18px; }
  /* Upcoming is not overdue: a calm surface card, not the alarming red used for `due`. */
  .banner.soon { background: var(--surface); color: var(--text); border: 1px solid var(--border); }
  .banner.soon a { color: var(--text); }
  .snooze { font-size: .8rem; padding: 2px 6px; }
</style>
