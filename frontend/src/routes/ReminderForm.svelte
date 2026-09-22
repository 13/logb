<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import DateInput from '../lib/DateInput.svelte';
  import { previewDates } from '../lib/recurrence';
  import { fmtDate } from '../lib/format';
  import { dateFormat } from '../stores/date-format';
  import { api, createReminderQueued } from '../lib/api';
  import { go, back } from '../lib/router';
  import { t } from '../i18n';
  import { emptyReminder, readingReminder, reminderBody, toReminderInput, validateReminder } from '../lib/reminder-form';
  import { fieldError } from '../lib/form-error';
  import type { MemObject, Reminder, ReminderInput } from '../lib/types';

  let { id, rid }: { id: string; rid?: string } = $props();
  const oid = $derived(Number(id));
  const editing = $derived(rid !== undefined);
  // `?kind=reading` is how "Remind me to log the reading" opens this form already switched over.
  const presetKind = new URLSearchParams(location.search).get('kind');
  let object = $state<MemObject | null>(null);
  let input = $state<ReminderInput>(emptyReminder());
  let error = $state('');
  let busy = $state(false);
  let recurrence = $state<'none' | 'interval' | 'daily' | 'weekly' | 'monthly' | 'yearly'>('none');
  let weekday = $state(1);
  let monthDay = $state<'last' | number>(1);
  let yearMonth = $state(1);
  let yearDay = $state(1);
  const upcoming = $derived(previewDates(input.schedule, input.due_date));

  function readSchedule() {
    if (input.repeat_months) {
      input.every_n = input.repeat_months; input.every_unit = 'month'; input.repeat_months = null;
    }
    if (input.repeat_months || input.every_n) recurrence = 'interval';
    else if (input.schedule === 'daily') recurrence = 'daily';
    else if (input.schedule?.startsWith('weekly:')) { recurrence = 'weekly'; weekday = Number(input.schedule.split(':')[1]); }
    else if (input.schedule?.startsWith('monthly:')) { recurrence = 'monthly'; const d = input.schedule.split(':')[1]; monthDay = d === 'last' ? 'last' : Number(d); }
    else if (input.schedule?.startsWith('yearly:')) { recurrence = 'yearly'; [, yearMonth, yearDay] = input.schedule.split(':').map(Number); }
    else recurrence = 'none';
  }

  function writeSchedule() {
    input.repeat_months = null;
    input.every_n = recurrence === 'interval' ? (input.every_n ?? 1) : null;
    input.every_unit = input.every_n ? (input.every_unit ?? 'month') : null;
    input.schedule = recurrence === 'daily' ? 'daily'
      : recurrence === 'weekly' ? `weekly:${weekday}`
      : recurrence === 'monthly' ? `monthly:${monthDay}`
      : recurrence === 'yearly' ? `yearly:${yearMonth}:${yearDay}` : null;
  }

  onMount(async () => {
    object = await api<MemObject>('GET', `/objects/${oid}`);
    if (rid) { input = toReminderInput(await api<Reminder>('GET', `/reminders/${rid}`)); readSchedule(); }
    else if (presetKind === 'reading' && (object.counter_unit || object.type === 'body')) { input = readingReminder($t(object?.type === 'body' ? 'weight.reminder' : 'reading.reminder-title')); readSchedule(); }
  });

  /** Switching kind on a new reminder. A title the user has not touched follows the kind, so
   *  "Log the counter reading" does not stay behind on a reminder that is now about brakes. */
  function setKind(kind: ReminderInput['kind']) {
    const readingTitle = $t(object?.type === 'body' ? 'weight.reminder' : 'reading.reminder-title');
    if (kind === 'reading') {
      input = { ...input, kind, every_n: input.every_n ?? 1, every_unit: input.every_unit ?? 'month', title: input.title || readingTitle };
    } else {
      input = { ...input, kind, title: input.title === readingTitle ? '' : input.title };
    }
    if (kind === 'reading' && recurrence === 'none') recurrence = 'interval';
    writeSchedule();
  }

  function num(v: number | null): number | null {
    return v === null || String(v) === '' ? null : Number(v);
  }

  function normalized(): ReminderInput {
    return reminderBody({
      ...input,
      due_date: input.due_date || null,
      due_counter: num(input.due_counter),
      repeat_months: num(input.repeat_months),
      repeat_counter: num(input.repeat_counter),
      every_n: num(input.every_n),
    });
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const body = normalized();
    const bad = validateReminder(body);
    if (bad) { error = fieldError(bad, $t); return; }
    busy = true; error = '';
    try {
      if (rid) await api('PATCH', `/reminders/${rid}`, body);
      else await createReminderQueued(`/objects/${oid}/reminders`, body as unknown as Record<string, unknown>);
      go(`/objects/${oid}?tab=reminders`, true);
    } catch (err) { error = (err as Error).message; } finally { busy = false; }
  }

  async function remove() {
    if (!rid || !confirm($t('nav.confirm-delete'))) return;
    await api('DELETE', `/reminders/${rid}`);
    go(`/objects/${oid}?tab=reminders`, true);
  }
</script>

<main>
  <TopBar title={editing ? $t('reminder.edit') : $t('reminder.new')} backTo={`/objects/${oid}?tab=reminders`} />
  <form onsubmit={submit}>
    <!-- A reading needs a counter to read, and a reminder keeps its kind once saved (the server
         refuses a change), so the choice is only offered where it can be made. -->
    {#if !editing && (object?.counter_unit || object?.type === 'body')}
      <fieldset class="field kind">
        <legend>{$t('reminder.kind')}</legend>
        <label class="row toggle"><input type="radio" name="kind" checked={input.kind === 'service'} onchange={() => setKind('service')} /> {$t('reminder.kind-service')}</label>
        <label class="row toggle"><input type="radio" name="kind" checked={input.kind === 'reading'} onchange={() => setKind('reading')} /> {$t(object?.type === 'body' ? 'weight.log' : 'reminder.kind-reading')}</label>
      </fieldset>
    {/if}
    <div class="field"><label for="ti">{$t('reminder.title')}</label><input id="ti" bind:value={input.title} required /></div>
      <div class="field">
        <label for="recurrence">{$t('reminder.recurrence')}</label>
        <select id="recurrence" bind:value={recurrence} onchange={writeSchedule}>
          {#if input.kind === 'service'}<option value="none">{$t('reminder.recurrence-none')}</option>{/if}
          <option value="interval">{$t('reminder.recurrence-interval')}</option>
          <option value="daily">{$t('reminder.recurrence-daily')}</option>
          <option value="weekly">{$t('reminder.recurrence-weekly')}</option>
          <option value="monthly">{$t('reminder.recurrence-monthly')}</option>
          <option value="yearly">{$t('reminder.recurrence-yearly')}</option>
        </select>
      </div>
      {#if recurrence === 'weekly'}
        <div class="field"><label for="weekday">{$t('reminder.weekday')}</label><select id="weekday" bind:value={weekday} onchange={writeSchedule}>{#each [1,2,3,4,5,6,7] as d}<option value={d}>{$t(`weekday.${d}`)}</option>{/each}</select></div>
      {:else if recurrence === 'monthly'}
        <div class="field"><label for="monthday">{$t('reminder.month-day')}</label><select id="monthday" bind:value={monthDay} onchange={writeSchedule}>{#each Array.from({length:31},(_,i)=>i+1) as d}<option value={d}>{d}</option>{/each}<option value="last">{$t('reminder.last-day')}</option></select></div>
      {:else if recurrence === 'yearly'}
        <div class="row">
          <div class="field"><label for="yearmonth">{$t('reminder.month')}</label><select id="yearmonth" bind:value={yearMonth} onchange={writeSchedule}>{#each Array.from({length:12},(_,i)=>i+1) as m}<option value={m}>{$t(`month.${m}`)}</option>{/each}</select></div>
          <div class="field"><label for="yearday">{$t('reminder.month-day')}</label><select id="yearday" bind:value={yearDay} onchange={writeSchedule}>{#each Array.from({length:31},(_,i)=>i+1) as d}<option value={d}>{d}</option>{/each}</select></div>
        </div>
      {/if}
      {#if input.schedule}
        <div class="field"><label for="schedule-start">{$t('reminder.starts')}</label><DateInput id="schedule-start" bind:value={() => input.due_date ?? '', (v) => (input.due_date = v || null)} /></div>
        <p class="hint">{$t('reminder.calendar-hint')}</p>
        <p aria-live="polite">{$t('reminder.preview')}: {upcoming.map(d => fmtDate(d, $dateFormat)).join(' · ')}</p>
      {/if}
      {#if recurrence === 'interval'}
        <div class="row">
          <div class="field"><label for="en">{$t('reminder.every')}</label><input id="en" type="number" min="1" max="60" bind:value={input.every_n} /></div>
          <div class="field"><label for="eu">{$t('reminder.every-unit')}</label><select id="eu" bind:value={input.every_unit}><option value="week">{$t('reminder.unit-week')}</option><option value="month">{$t('reminder.unit-month')}</option></select></div>
        </div>
      {/if}
    {#if input.kind === 'reading'}
      {#if !input.schedule}<div class="field"><label for="st">{$t('reminder.starts')}</label><DateInput id="st" bind:value={() => input.due_date ?? '', (v) => (input.due_date = v || null)} /></div>{/if}
      <p class="hint">{$t(object?.type === 'body' ? 'weight.reminder-hint' : 'reminder.reading-hint')}</p>
    {:else}
      <div class="row">
        {#if !input.schedule}<div class="field"><label for="dd">{$t('reminder.due-date')}</label><DateInput id="dd" bind:value={() => input.due_date ?? '', (v) => (input.due_date = v || null)} /></div>{/if}
        {#if object?.counter_unit}
          <div class="field"><label for="dc">{$t('reminder.due-counter')} ({object.counter_unit})</label><input id="dc" type="number" min="0" bind:value={input.due_counter} /></div>
        {/if}
      </div>
      <div class="row">
        {#if object?.counter_unit}
          <div class="field"><label for="rc">{$t('reminder.repeat-counter')} ({object.counter_unit})</label><input id="rc" type="number" min="1" bind:value={input.repeat_counter} /></div>
        {/if}
      </div>
    {/if}
    <div class="field"><label for="no">{$t('reminder.notes')}</label><textarea id="no" bind:value={input.notes}></textarea></div>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="row actions">
      <button type="button" class="ghost" onclick={() => back(`/objects/${oid}?tab=reminders`)}>{$t('nav.cancel')}</button>
      <button class="primary" disabled={busy}>{$t('nav.save')}</button>
    </div>
  </form>
  {#if editing}<button class="danger" onclick={remove}>{$t('nav.delete')}</button>{/if}
</main>

<style>
  .actions { margin-top: var(--space-2); }
  .kind { border: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: var(--space-2); }
  .kind legend { padding: 0; margin-bottom: var(--space-1); }
</style>
