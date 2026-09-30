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
  import { Button } from '$lib/components/ui/button/index.js';
  import TagChips from './TagChips.svelte';
  import type { Activity, CounterUnit, DoneOut, Reminder } from './types';

  let { objectId, unit, activities, onchanged, onloaded, body = false }:
    { objectId: number; body?: boolean; unit: CounterUnit; activities: Activity[]; onchanged?: () => void; onloaded?: (rows: Reminder[]) => void } = $props();

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
      onloaded?.(rows);
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
{#if toast}<p class="m-0 mb-3 text-sm text-muted-foreground">{toast}</p>{/if}

<!-- Not `items.length === 0`: an empty list before the first answer is what the component was
     initialised with, not what the server said. -->
{#if loaded && items.length === 0}
  <div class="flex flex-col items-center gap-3 px-4 py-10 text-center">
    <span class="text-muted-foreground opacity-40"><Icon name="repeat" size={40} /></span>
    <p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('reminder.empty')}</p>
    <Button class="min-h-11" onclick={() => go(`/objects/${objectId}/reminders/new`)}>+ {$t('reminder.new')}</Button>
    {#if unit || body}
      <Button variant="outline" class="min-h-11" onclick={() => go(`/objects/${objectId}/reminders/new?kind=reading`)}>{$t(body ? 'weight.reminder' : 'reminder.new-reading')}</Button>
    {/if}
  </div>
{/if}

<!-- A due reminder's card is tinted like the dashboard's, so what needs doing reads first. -->
<ul role="list" class="m-0 flex list-none flex-col gap-2 p-0 pb-20">
  {#each [...groups.due, ...groups.open] as r (r.id)}
    <li data-testid="reminder-card"
        class={['flex flex-col gap-1 rounded-lg border p-3 shadow-xs', r.due ? 'border-destructive/30 bg-destructive/10' : 'border-border bg-card']}>
      <div class="flex items-start gap-2">
        <span class="min-w-0 flex-1 break-words font-semibold text-foreground">{r.title}</span>
        {#if r.pending}
          <!-- Only in the outbox so far: the server has not computed whether it is due. -->
          <span class="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('timeline.pending')}</span>
        {:else}
          <span data-testid="reminder-status"
                class={['shrink-0 rounded-full px-2 py-0.5 text-xs font-semibold', r.due ? 'bg-destructive text-destructive-foreground' : 'bg-muted text-muted-foreground']}>
            {r.due ? $t('reminder.due') : r.snoozed_until ? $t('reminder.snoozed') : $t('reminder.open')}
          </span>
        {/if}
      </div>
      {#if (r.object_tags ?? []).length > 0}<div class="w-fit"><TagChips tags={r.object_tags ?? []} /></div>{/if}
      {#if r.kind === 'reading'}
        <p class="m-0 flex items-center gap-1 text-sm text-muted-foreground"><span class="inline-flex" role="img" aria-label={$t('activity.repeat')}><Icon name="repeat" size={14} /></span> {every(r)}</p>
        <p class="m-0 text-sm text-muted-foreground tabular-nums">{reading(r)}</p>
      {:else}
        <p class="m-0 text-sm text-muted-foreground">
          {when(r)}{#if r.repeat_months || r.repeat_counter}{' · '}<span class="inline-flex align-middle" role="img" aria-label={$t('activity.repeat')}><Icon name="repeat" size={14} /></span>{/if}
        </p>
        {#if r.estimated_due_date}
          <p class="m-0 text-sm text-muted-foreground">{$t('reminder.estimated', { date: fmtDate(r.estimated_due_date, $dateFormat) })}</p>
        {/if}
      {/if}
      {#if !r.due && r.snoozed_until}
        <!-- `due_date`/`due_counter` never change on snooze (see src/api/reminders.rs), so the line
             above can still read as overdue while the reminder is suppressed -- this one says so. -->
        <div class="mt-1 flex items-center gap-2">
          <span class="min-w-0 flex-1 text-sm text-muted-foreground">{$t('reminder.snoozed-until', { date: fmtDate(r.snoozed_until, $dateFormat) })}</span>
          <Button variant="outline" class="min-h-11 shrink-0" onclick={() => unsnooze(r)}>{$t('reminder.unsnooze')}</Button>
        </div>
      {/if}
      {#if r.notes}<p class="m-0 mt-1 whitespace-pre-wrap text-sm text-foreground">{r.notes}</p>{/if}
      <!-- A pending reminder has no server row yet, so nothing here could address it. -->
      {#if !r.pending}
        <div class="mt-2 flex flex-wrap gap-2">
          <Button variant="outline" class="min-h-11" onclick={() => go(`/objects/${objectId}/reminders/${r.id}`)}>{$t('nav.edit')}</Button>
          {#if r.kind === 'reading'}
            <!-- No "done": logging the reading is what satisfies it, from here or anywhere else. -->
            {#if r.due}<Button variant="outline" class="min-h-11" onclick={() => skip(r)}>{$t('reminder.skip')}</Button>{/if}
            <Button class="min-h-11" onclick={() => go(`/objects/${objectId}/reading`)}>{$t(body ? 'weight.log' : 'reminder.record')}</Button>
          {:else}
            <Button class="min-h-11" onclick={() => openDone(r)}>{$t('reminder.mark-done')}</Button>
          {/if}
        </div>
      {/if}
    </li>
  {/each}
</ul>

{#if groups.done.length > 0}
  <Button variant="ghost" class="mt-4 min-h-11 w-full justify-start text-muted-foreground" aria-expanded={showDone} onclick={() => (showDone = !showDone)}>
    {showDone ? '▾' : '▸'} {$t('reminder.history')} ({groups.done.length})
  </Button>
  {#if showDone}
    <!-- Muted fill rather than faded opacity: faded text would drop below 4.5:1. -->
    <ul role="list" class="m-0 mt-2 flex list-none flex-col gap-2 p-0">
      {#each groups.done as r (r.id)}
        <li data-testid="reminder-done" class="flex flex-col gap-0.5 rounded-lg bg-muted p-3">
          <span class="font-semibold text-foreground">{r.title}</span>
          <span class="text-sm text-muted-foreground">{$t('reminder.done')} · {fmtDate(r.done_at, $dateFormat)}</span>
        </li>
      {/each}
    </ul>
  {/if}
{/if}

<!-- The empty state carries this same action, so only one of the two is ever on screen. -->
{#if items.length > 0}
  <Button class="fab-pos h-12 rounded-full px-5 text-base font-semibold shadow-lg" onclick={() => go(`/objects/${objectId}/reminders/new`)}>+ {$t('reminder.new')}</Button>
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
