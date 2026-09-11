<script lang="ts">
  import { api } from './api';
  import { go } from './router';
  import { counter, fmtDate } from './format';
  import { locale, t } from '../i18n';
  import { splitReminders } from './reminder-form';
  import Icon from './Icon.svelte';
  import type { Activity, CounterUnit, DoneOut, Reminder } from './types';

  let { objectId, unit, activities, onchanged }:
    { objectId: number; unit: CounterUnit; activities: Activity[]; onchanged?: () => void } = $props();

  let items = $state<Reminder[]>([]);
  let showDone = $state(false);
  let dialog = $state<HTMLDialogElement | null>(null);
  let target = $state<Reminder | null>(null);
  let linkId = $state<string>('');
  let toast = $state('');
  let error = $state('');

  const groups = $derived(splitReminders(items));
  // A pending (queued-offline) timeline entry has a synthetic negative id -- the server has
  // never heard of it. Offering one in this dropdown lets a user "link" a reminder to it,
  // which POSTs `activity_id: <negative number>` and 404s. Excluded here rather than filtered
  // by the caller, since every consumer of this dropdown must exclude them the same way.
  const linkable = $derived(activities.filter((a) => !a.pending));

  async function load() {
    try { items = await api<Reminder[]>('GET', `/objects/${objectId}/reminders`); }
    catch (e) { error = (e as Error).message; }
  }
  $effect(() => { objectId; load(); });

  function openDone(r: Reminder) { target = r; linkId = ''; dialog?.showModal(); }

  async function confirmDone() {
    if (!target) return;
    try {
      const body = linkId === '' ? {} : { activity_id: Number(linkId) };
      const res = await api<DoneOut>('POST', `/reminders/${target.id}/done`, body);
      toast = res.next ? $t('reminder.next-created') : '';
      dialog?.close();
      await load();
      onchanged?.();
    } catch (e) { error = (e as Error).message; }
  }

  async function unsnooze(r: Reminder) {
    try {
      await api<Reminder>('DELETE', `/reminders/${r.id}/snooze`);
      await load();
      onchanged?.();
    } catch (e) { error = (e as Error).message; }
  }

  function when(r: Reminder): string {
    const parts: string[] = [];
    if (r.due_date) parts.push($t('reminder.on', { date: fmtDate(r.due_date, $locale) }));
    if (r.due_counter !== null) parts.push($t('reminder.at', { counter: counter(r.due_counter, unit, $locale) }));
    return parts.join(' · ');
  }
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if toast}<p class="muted">{toast}</p>{/if}

{#if items.length === 0}
  <p class="muted">{$t('reminder.empty')}</p>
{/if}

<div class="list">
  {#each [...groups.due, ...groups.open] as r (r.id)}
    <div class="card">
      <div class="row head">
        <b>{r.title}</b>
        <span class="chip" class:due={r.due} class:snoozed={!r.due && r.snoozed_until}>
          {r.due ? $t('reminder.due') : r.snoozed_until ? $t('reminder.snoozed') : $t('reminder.open')}
        </span>
      </div>
      <div class="muted">{when(r)}{#if r.repeat_months || r.repeat_counter} · <span class="repeat-icon" role="img" aria-label={$t('activity.repeat')}><Icon name="repeat" size={14} /></span>{/if}</div>
      {#if !r.due && r.snoozed_until}
        <!-- `due_date`/`due_counter` never change on snooze (see src/api/reminders.rs), so
             `when(r)` above can still read as overdue while the reminder is suppressed -- this
             line is what actually says so. -->
        <div class="row snoozed-until">
          <span class="muted">{$t('reminder.snoozed-until', { date: fmtDate(r.snoozed_until, $locale) })}</span>
          <button class="ghost" onclick={() => unsnooze(r)}>{$t('reminder.unsnooze')}</button>
        </div>
      {/if}
      {#if r.notes}<p class="notes">{r.notes}</p>{/if}
      <div class="row actions">
        <button class="ghost" onclick={() => go(`/objects/${objectId}/reminders/${r.id}`)}>{$t('nav.edit')}</button>
        <button class="primary" onclick={() => openDone(r)}>{$t('reminder.mark-done')}</button>
      </div>
    </div>
  {/each}
</div>

{#if groups.done.length > 0}
  <button class="ghost more" onclick={() => (showDone = !showDone)}>{showDone ? '▾' : '▸'} {$t('reminder.history')} ({groups.done.length})</button>
  {#if showDone}
    <div class="list">
      {#each groups.done as r (r.id)}
        <div class="card done">
          <b>{r.title}</b>
          <div class="muted">{$t('reminder.done')} · {fmtDate(r.done_at, $locale)}</div>
        </div>
      {/each}
    </div>
  {/if}
{/if}

<button class="primary fab" onclick={() => go(`/objects/${objectId}/reminders/new`)}>+ {$t('reminder.new')}</button>

<dialog bind:this={dialog}>
  <h2>{$t('reminder.done-title')}</h2>
  <div class="field">
    <label for="link">{$t('reminder.done-link')}</label>
    <select id="link" bind:value={linkId}>
      <option value="">{$t('reminder.done-none')}</option>
      {#each linkable as a (a.id)}
        <option value={String(a.id)}>{fmtDate(a.date, $locale)} — {a.title}</option>
      {/each}
    </select>
  </div>
  <div class="row">
    <button class="ghost" onclick={() => dialog?.close()}>{$t('nav.cancel')}</button>
    <button class="primary" onclick={confirmDone}>{$t('reminder.done')}</button>
  </div>
</dialog>

<style>
  .head { justify-content: space-between; }
  .head b { flex: 1; }
  .head .chip { flex: none; }
  .chip.snoozed { background: var(--surface-2); color: var(--muted); }
  .snoozed-until { margin-top: 2px; justify-content: space-between; }
  .snoozed-until span { flex: 1; }
  .snoozed-until button { flex: none; }
  .notes { font-size: .9rem; white-space: pre-wrap; margin-top: 4px; }
  .repeat-icon { display: inline-flex; vertical-align: -2px; }
  .actions { margin-top: 8px; }
  .more { margin-top: 16px; width: 100%; text-align: left; color: var(--muted); }
  .done { opacity: .7; }
</style>
