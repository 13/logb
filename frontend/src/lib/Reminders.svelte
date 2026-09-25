<script lang="ts">
  import { errorMessage } from './api-error';
  import { untrack } from 'svelte';
  import { api, onOutboxFlushed, pendingOpsFor } from './api';
  import { createSeq } from './seq-guard';
  import { pendingReminders } from './reminder-pending';
  import { go } from './router';
  import { counter, fmtDate } from './format';
  import { activityTitle } from './activity-form';
  import { dateFormat } from '../stores/date-format';
  import { locale, t } from '../i18n';
  import { splitReminders } from './reminder-form';
  import Icon from './Icon.svelte';
  import TagChips from './TagChips.svelte';
  import type { Activity, CounterUnit, DoneOut, Reminder } from './types';

  let { objectId, unit, activities, onchanged, body = false }:
    { objectId: number; body?: boolean; unit: CounterUnit; activities: Activity[]; onchanged?: () => void } = $props();

  let items = $state<Reminder[]>([]);
  /** Whether the answer is known -- see Documents.svelte for why an empty list is not one. */
  let loaded = $state(false);
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

  /// ObjectDetail reuses this instance across objects (only `objectId` changes): only the
  /// newest load may commit, so the object just left cannot land its reminders on this one.
  const loadSeq = createSeq();

  async function load() {
    const token = loadSeq.next();
    const oid = objectId;
    // Queued creates are read first and apart: a blocked IndexedDB must not hide the server's
    // list, and a dead connection must not hide what is waiting to be sent.
    let queued: Reminder[] = [];
    try { queued = pendingReminders(await pendingOpsFor(`/objects/${oid}/reminders`), oid); } catch { /* see above */ }
    try {
      const rows = await api<Reminder[]>('GET', `/objects/${oid}/reminders`);
      if (!loadSeq.current(token)) return;
      items = [...queued, ...rows];
      error = '';
    } catch (e) {
      if (!loadSeq.current(token)) return;
      items = [...queued, ...items.filter((r) => !r.pending)];
      error = errorMessage(e, $t);
    } finally {
      if (loadSeq.current(token)) loaded = true;
    }
  }
  // A new object starts unknown, not with the last one's reminders, toast or error.
  $effect(() => {
    objectId;
    untrack(() => { items = []; loaded = false; error = ''; toast = ''; void load(); });
  });
  // A queued reminder reaching the server turns from pending into real without a remount.
  $effect(() => onOutboxFlushed((_resolved, changed) => { if (changed) void load(); }));

  /** Mark-done's own state, shown inside the dialog: an error rendered behind a modal dialog
   *  is one nobody sees until they cancel. */
  let doneBusy = $state(false);
  let doneError = $state('');

  function openDone(r: Reminder) { target = r; linkId = ''; doneError = ''; dialog?.showModal(); }

  async function confirmDone() {
    if (!target || doneBusy) return;
    doneBusy = true; doneError = '';
    try {
      const body = linkId === '' ? {} : { activity_id: Number(linkId) };
      const res = await api<DoneOut>('POST', `/reminders/${target.id}/done`, body);
      toast = res.next ? $t('reminder.next-created') : '';
      dialog?.close();
      await load();
      onchanged?.();
    } catch (e) { doneError = errorMessage(e, $t); }
    finally { doneBusy = false; }
  }

  async function unsnooze(r: Reminder) {
    try {
      await api<Reminder>('DELETE', `/reminders/${r.id}/snooze`);
      await load();
      onchanged?.();
    } catch (e) { error = errorMessage(e, $t); }
  }

  /** "Skip this one": a snooze one interval long, so the reading is asked for again next period
   *  rather than tomorrow. The server measures it from today when the reading is overdue. */
  async function skip(r: Reminder) {
    try {
      await api<Reminder>('POST', `/reminders/${r.id}/snooze`, { skip: true });
      await load();
      onchanged?.();
    } catch (e) { error = errorMessage(e, $t); }
  }

  function when(r: Reminder): string {
    const parts: string[] = [];
    if (r.schedule) {
      const p = r.schedule.split(':');
      if (p[0] === 'daily') parts.push($t('reminder.recurrence-daily'));
      if (p[0] === 'weekly') parts.push($t('reminder.schedule-weekly', { day: $t(`weekday.${p[1]}`) }));
      if (p[0] === 'monthly') parts.push(p[1] === 'last' ? $t('reminder.last-day') : $t('reminder.schedule-monthly', { day: p[1] }));
      if (p[0] === 'yearly') parts.push($t('reminder.schedule-yearly', { month: $t(`month.${p[1]}`), day: p[2] }));
    }
    if (r.due_date) parts.push($t('reminder.on', { date: fmtDate(r.due_date, $dateFormat) }));
    if (r.due_counter !== null) parts.push($t('reminder.at', { counter: counter(r.due_counter, unit, $locale) }));
    return parts.join(' · ');
  }

  function every(r: Reminder): string {
    if (r.schedule) return when({ ...r, due_date: null });
    const n = r.every_n ?? 1;
    if (r.every_unit === 'week') return n === 1 ? $t('reminder.every-week') : $t('reminder.every-weeks', { n });
    return n === 1 ? $t('reminder.every-month') : $t('reminder.every-months', { n });
  }

  function reading(r: Reminder): string {
    if (body) return r.last_reading_date ? `${$t('weight.last-date')}: ${fmtDate(r.last_reading_date, $dateFormat)}` : $t('weight.empty');
    const last = r.last_reading_date && r.current_counter !== null
      ? $t('reminder.last-reading', { counter: counter(r.current_counter, unit, $locale), date: fmtDate(r.last_reading_date, $dateFormat) })
      : $t('reminder.no-reading');
    const next = r.next_due_date && !r.due ? ` · ${$t('reminder.next-reading', { date: fmtDate(r.next_due_date, $dateFormat) })}` : '';
    return `${last}${next}`;
  }
</script>

{#if error}<p class="error" role="alert">{error}</p>{/if}
{#if toast}<p class="muted">{toast}</p>{/if}

<!-- Not `items.length === 0`: an empty list before the first answer is what the component was
     initialised with, not what the server said, and the empty state is a whole block -- icon,
     sentence and a primary button -- to flash and take away. -->
{#if loaded && items.length === 0}
  <div class="empty">
    <span class="empty-icon"><Icon name="repeat" size={40} /></span>
    <p>{$t('reminder.empty')}</p>
    <button class="primary" onclick={() => go(`/objects/${objectId}/reminders/new`)}>+ {$t('reminder.new')}</button>
    {#if unit || body}
      <button class="ghost" onclick={() => go(`/objects/${objectId}/reminders/new?kind=reading`)}>{$t(body ? 'weight.reminder' : 'reminder.new-reading')}</button>
    {/if}
  </div>
{/if}

<div class="list">
  {#each [...groups.due, ...groups.open] as r (r.id)}
    <div class="card">
      <div class="row head">
        <b>{r.title}</b>
        {#if r.pending}
          <!-- Only in the outbox so far: the server has not computed whether it is due. -->
          <span class="chip pending-chip">{$t('timeline.pending')}</span>
        {:else}
          <span class="chip" class:due={r.due} class:snoozed={!r.due && r.snoozed_until}>
            {r.due ? $t('reminder.due') : r.snoozed_until ? $t('reminder.snoozed') : $t('reminder.open')}
          </span>
        {/if}
      </div>
      <TagChips tags={r.object_tags ?? []} />
      {#if r.kind === 'reading'}
        <div class="muted"><span class="repeat-icon" role="img" aria-label={$t('activity.repeat')}><Icon name="repeat" size={14} /></span> {every(r)}</div>
        <div class="muted tnum">{reading(r)}</div>
      {:else}
        <div class="muted">{when(r)}{#if r.repeat_months || r.repeat_counter} · <span class="repeat-icon" role="img" aria-label={$t('activity.repeat')}><Icon name="repeat" size={14} /></span>{/if}</div>
        {#if r.estimated_due_date}
          <div class="muted">{$t('reminder.estimated', { date: fmtDate(r.estimated_due_date, $dateFormat) })}</div>
        {/if}
      {/if}
      {#if !r.due && r.snoozed_until}
        <!-- `due_date`/`due_counter` never change on snooze (see src/api/reminders.rs), so
             `when(r)` above can still read as overdue while the reminder is suppressed -- this
             line is what actually says so. -->
        <div class="row snoozed-until">
          <span class="muted">{$t('reminder.snoozed-until', { date: fmtDate(r.snoozed_until, $dateFormat) })}</span>
          <button class="ghost" onclick={() => unsnooze(r)}>{$t('reminder.unsnooze')}</button>
        </div>
      {/if}
      {#if r.notes}<p class="notes">{r.notes}</p>{/if}
      <!-- A pending reminder has no server row yet, so nothing here could address it. -->
      {#if !r.pending}
      <div class="row actions">
        <button class="ghost" onclick={() => go(`/objects/${objectId}/reminders/${r.id}`)}>{$t('nav.edit')}</button>
        {#if r.kind === 'reading'}
          <!-- No "done": logging the reading is what satisfies it, from here or anywhere else. -->
          {#if r.due}<button class="ghost" onclick={() => skip(r)}>{$t('reminder.skip')}</button>{/if}
          <button class="primary" onclick={() => go(`/objects/${objectId}/reading`)}>{$t(body ? 'weight.log' : 'reminder.record')}</button>
        {:else}
          <button class="primary" onclick={() => openDone(r)}>{$t('reminder.mark-done')}</button>
        {/if}
      </div>
      {/if}
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
          <div class="muted">{$t('reminder.done')} · {fmtDate(r.done_at, $dateFormat)}</div>
        </div>
      {/each}
    </div>
  {/if}
{/if}

<!-- The empty state carries this same action, so only one of the two is ever on screen. -->
{#if items.length > 0}
  <button class="primary fab" onclick={() => go(`/objects/${objectId}/reminders/new`)}>+ {$t('reminder.new')}</button>
{/if}

<dialog bind:this={dialog} aria-labelledby="reminder-done-title">
  <h2 id="reminder-done-title">{$t('reminder.done-title')}</h2>
  {#if doneError}<p class="error" role="alert">{doneError}</p>{/if}
  <div class="field">
    <label for="link">{$t('reminder.done-link')}</label>
    <select id="link" bind:value={linkId}>
      <option value="">{$t('reminder.done-none')}</option>
      {#each linkable as a (a.id)}
        <option value={String(a.id)}>{fmtDate(a.date, $dateFormat)} — {activityTitle(a.title, a.category, $t)}</option>
      {/each}
    </select>
  </div>
  <div class="row">
    <button class="ghost" onclick={() => dialog?.close()}>{$t('nav.cancel')}</button>
    <button class="primary" onclick={confirmDone} disabled={doneBusy}>{$t('reminder.done')}</button>
  </div>
</dialog>

<style>
  .head { justify-content: space-between; }
  .head b { flex: 1; }
  .head .chip { flex: none; }
  .chip.snoozed { background: var(--surface-2); color: var(--muted); }
  .snoozed-until { margin-top: var(--space-1); justify-content: space-between; }
  .snoozed-until span { flex: 1; }
  .snoozed-until button { flex: none; }
  .notes { font-size: var(--text-sm); white-space: pre-wrap; margin-top: var(--space-1); }
  .repeat-icon { display: inline-flex; vertical-align: -2px; }
  .actions { margin-top: var(--space-2); }
  .more { margin-top: var(--space-4); width: 100%; text-align: left; color: var(--muted); }
  .done { opacity: .7; }
</style>
