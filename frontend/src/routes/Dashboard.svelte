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
      // The archived view is the one screen whose job is "where archived things live", so it
      // asks for archived objects at any depth, flat: `all=true` means "ignore nesting" and
      // says nothing about `archived`, which stays an independent either/or filter. Without it
      // the view returns archived *roots* only, and an archived object inside a room appears in
      // no list in the app at all. The live view keeps the roots-only default, because there
      // the nesting is the point -- a room is reached through the house that holds it.
      const scope = archived ? 'archived=true&all=true' : 'archived=false';
      objects = await api<MemObject[]>('GET', `/objects?${scope}`);
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
  <TopBar title={$t('dash.title')} />

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

  <!-- The archived filter is a chip, like the category chips on an object: a control that
       narrows a list belongs above the list in the row of such controls, not as a loose
       checkbox trailing off the bottom of the page. `aria-pressed` carries the on/off state a
       checkbox used to carry, and the label is unchanged. -->
  <div class="chips">
    <button
      class="chip"
      class:active={archived}
      aria-pressed={archived}
      onclick={() => (archived = !archived)}
    >{$t('dash.show-archived')}</button>
  </div>

  {#if error}<p class="error">{error}</p>{/if}
  {#if loading}
    <p class="muted">{$t('nav.loading')}</p>
  {:else if objects.length === 0}
    <!-- The one screen in the app that can say what LogB is for: it is what a new user sees
         the moment setup finishes. The archived view is a filter, not a first run, so it gets
         the fact instead of the pitch. -->
    <div class="empty">
      {#if archived}
        <p>{$t('dash.none-archived')}</p>
      {:else}
        <span class="empty-icon"><Icon name="object" size={40} /></span>
        <p>{$t('dash.empty')}</p>
        <button class="primary" onclick={() => go('/objects/new')}>+ {$t('dash.new')}</button>
      {/if}
    </div>
  {:else}
    <div class="list">
      {#each objects as o (o.id)}<ObjectCard object={o} />{/each}
    </div>
  {/if}

  <!-- Hidden while the empty state is showing: that state carries the same action as its own
       call to action, and two buttons named "New object" on one screen is one too many -- for
       a reader and for anything resolving that name. -->
  {#if objects.length > 0 || archived}
    <button class="primary fab" onclick={() => go('/objects/new')}>+ {$t('dash.new')}</button>
  {/if}
</main>

<style>
  .banner ul { margin: var(--space-2) 0 0 var(--space-4); }
  /* Upcoming is not overdue: a calm surface card, not the alarming red used for `due`. */
  .banner.soon { background: var(--surface); color: var(--text); border: 1px solid var(--border); }
  .banner.soon a { color: var(--text); }
  .snooze { font-size: var(--text-xs); padding: 2px var(--space-2); }
</style>
