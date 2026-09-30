<script lang="ts">
  import { errorMessage } from '../lib/api-error';
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import DateInput from '../lib/DateInput.svelte';
  import { previewDates } from '../lib/recurrence';
  import { fmtDate } from '../lib/format';
  import { dateFormat } from '../stores/date-format';
  import { api, createReminderQueued } from '../lib/api';
  import { go, back } from '../lib/router';
  import { t } from '../i18n';
  import { applyDueMode, dueModeOf, emptyReminder, readingReminder, reminderBody, REMINDER_FIELD_IDS, toReminderInput, validateReminder, type DueMode } from '../lib/reminder-form';
  import { fieldErrorAt, type FieldError } from '../lib/form-error';
  import FormActions from '../lib/FormActions.svelte';
  import MoreDetails from '../lib/MoreDetails.svelte';
  import { revealField } from '../lib/reveal-field';
  import { Field } from '$lib/components/ui/field/index.js';
  import { hintClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Textarea } from '$lib/components/ui/textarea/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { Segmented } from '$lib/components/ui/segmented/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
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

  /** Which due field a service reminder watches; only asked where the object has a counter. */
  let dueMode = $state<DueMode>('date');
  let moreOpen = $state(false);
  /** A save refused by `validateReminder`, shown under the field it names. */
  let fieldErr = $state<FieldError | null>(null);
  const errorFor = (fid: string): string => (fieldErr?.id === fid ? fieldErr.message : '');
  const formError = $derived(error || (fieldErr?.id === null ? fieldErr.message : ''));
  const hasCounter = $derived(!!object?.counter_unit);
  const showDate = $derived(input.kind === 'reading' || !hasCounter || dueMode !== 'counter');
  const showCounter = $derived(input.kind === 'service' && hasCounter && dueMode !== 'date');

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

  /** True until the object and the reminder being edited are applied (`aria-busy`). */
  let loading = $state(true);

  onMount(async () => {
    try {
      object = await api<MemObject>('GET', `/objects/${oid}`);
      if (rid) {
        input = toReminderInput(await api<Reminder>('GET', `/reminders/${rid}`));
        readSchedule();
        dueMode = dueModeOf(input, !!object.counter_unit);
        moreOpen = input.notes.trim() !== '';
      }
      else if (presetKind === 'reading' && (object.counter_unit || object.type === 'body')) { input = readingReminder($t(object?.type === 'body' ? 'weight.reminder' : 'reading.reminder-title')); readSchedule(); }
    } finally { loading = false; }
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
    const body = reminderBody({
      ...input,
      due_date: input.due_date || null,
      due_counter: num(input.due_counter),
      repeat_months: num(input.repeat_months),
      repeat_counter: num(input.repeat_counter),
      every_n: num(input.every_n),
    });
    // Without a counter there are no sides; a reminder saved when the object still had one keeps
    // its counter rather than losing it to an edit of its title.
    return hasCounter ? applyDueMode(body, dueMode) : body;
  }

  async function reject(key: string) {
    const ids = { ...REMINDER_FIELD_IDS, 'reminder.due-date': showDate ? 'dd' : 'dc' };
    const at = fieldErrorAt(key, $t, ids);
    fieldErr = at;
    if (at.id !== null && !(await revealField(at.id))) fieldErr = { id: null, message: at.message };
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    fieldErr = null;
    const body = normalized();
    const bad = validateReminder(body);
    if (bad) { await reject(bad); return; }
    busy = true; error = '';
    try {
      if (rid) await api('PATCH', `/reminders/${rid}`, body);
      else await createReminderQueued(`/objects/${oid}/reminders`, body as unknown as Record<string, unknown>);
      go(`/objects/${oid}?tab=reminders`, true);
    } catch (err) { error = errorMessage(err, $t); } finally { busy = false; }
  }

  async function remove() {
    if (!rid || busy || !confirm($t('nav.confirm-delete'))) return;
    busy = true; error = '';
    try {
      await api('DELETE', `/reminders/${rid}`);
      go(`/objects/${oid}?tab=reminders`, true);
    } catch (e) {
      error = errorMessage(e, $t);
    } finally { busy = false; }
  }
</script>

<main>
  <TopBar title={editing ? $t('reminder.edit') : $t('reminder.new')} backTo={`/objects/${oid}?tab=reminders`} />
  <form onsubmit={submit} aria-busy={loading} class="m-0 flex w-full max-w-[40rem] flex-col gap-5">
    <!-- A reading needs a counter to read, and a reminder keeps its kind once saved (the server
         refuses a change), so the choice is only offered where it can be made. -->
    {#if !editing && (object?.counter_unit || object?.type === 'body')}
      <Segmented legend={$t('reminder.kind')} name="kind" value={input.kind} onchange={setKind}
                 options={[
                   { value: 'service', label: $t('reminder.kind-service') },
                   { value: 'reading', label: $t(object?.type === 'body' ? 'weight.log' : 'reminder.kind-reading') },
                 ]} />
    {/if}
    <Field id="ti" label={$t('reminder.title')} error={errorFor('ti')}><Input bind:value={input.title} required /></Field>

    {#if input.kind === 'service' && hasCounter}
      <Segmented legend={$t('reminder.due-by')} name="due-by" bind:value={dueMode}
                 hint={dueMode === 'both' ? $t('reminder.due-by-both-hint') : ''}
                 options={[
                   { value: 'date', label: $t('reminder.due-by-date') },
                   { value: 'counter', label: $t('reminder.due-by-counter') },
                   { value: 'both', label: $t('reminder.due-by-both') },
                 ]} />
    {/if}

    {#if showDate}
      <Field id="recurrence" label={$t('reminder.recurrence')} error={errorFor('recurrence')}>
        <NativeSelect bind:value={() => recurrence, (v) => { recurrence = v; writeSchedule(); }}>
          {#if input.kind === 'service'}<option value="none">{$t('reminder.recurrence-none')}</option>{/if}
          <option value="interval">{$t('reminder.recurrence-interval')}</option>
          <option value="daily">{$t('reminder.recurrence-daily')}</option>
          <option value="weekly">{$t('reminder.recurrence-weekly')}</option>
          <option value="monthly">{$t('reminder.recurrence-monthly')}</option>
          <option value="yearly">{$t('reminder.recurrence-yearly')}</option>
        </NativeSelect>
      </Field>
      {#if recurrence === 'weekly'}
        <Field id="weekday" label={$t('reminder.weekday')}>
          <NativeSelect bind:value={() => weekday, (v) => { weekday = Number(v); writeSchedule(); }}>
            {#each [1, 2, 3, 4, 5, 6, 7] as d (d)}<option value={d}>{$t(`weekday.${d}`)}</option>{/each}
          </NativeSelect>
        </Field>
      {:else if recurrence === 'monthly'}
        <Field id="monthday" label={$t('reminder.month-day')}>
          <NativeSelect bind:value={() => monthDay, (v) => { monthDay = v; writeSchedule(); }}>
            {#each Array.from({ length: 31 }, (_, i) => i + 1) as d (d)}<option value={d}>{d}</option>{/each}
            <option value="last">{$t('reminder.last-day')}</option>
          </NativeSelect>
        </Field>
      {:else if recurrence === 'yearly'}
        <div class="grid grid-cols-2 gap-3">
          <Field id="yearmonth" label={$t('reminder.month')}>
            <NativeSelect bind:value={() => yearMonth, (v) => { yearMonth = Number(v); writeSchedule(); }}>
              {#each Array.from({ length: 12 }, (_, i) => i + 1) as m (m)}<option value={m}>{$t(`month.${m}`)}</option>{/each}
            </NativeSelect>
          </Field>
          <Field id="yearday" label={$t('reminder.month-day')}>
            <NativeSelect bind:value={() => yearDay, (v) => { yearDay = Number(v); writeSchedule(); }}>
              {#each Array.from({ length: 31 }, (_, i) => i + 1) as d (d)}<option value={d}>{d}</option>{/each}
            </NativeSelect>
          </Field>
        </div>
      {/if}
      {#if input.schedule}
        <Field id="schedule-start" label={$t('reminder.starts')} hint={$t('reminder.calendar-hint')}>
          <DateInput id="schedule-start" bind:value={() => input.due_date ?? '', (v) => (input.due_date = v || null)} />
        </Field>
        <p aria-live="polite" class="m-0 text-sm text-muted-foreground tabular-nums">{$t('reminder.preview')}: {upcoming.map((d) => fmtDate(d, $dateFormat)).join(' · ')}</p>
      {/if}
      {#if recurrence === 'interval'}
        <div class="grid grid-cols-2 gap-3">
          <Field id="en" label={$t('reminder.every')} error={errorFor('en')}><Input type="number" min="1" max="60" bind:value={input.every_n} /></Field>
          <Field id="eu" label={$t('reminder.every-unit')}>
            <NativeSelect bind:value={input.every_unit}>
              <option value="week">{$t('reminder.unit-week')}</option>
              <option value="month">{$t('reminder.unit-month')}</option>
            </NativeSelect>
          </Field>
        </div>
      {/if}
      {#if input.kind === 'reading'}
        {#if !input.schedule}
          <Field id="st" label={$t('reminder.starts')}><DateInput id="st" bind:value={() => input.due_date ?? '', (v) => (input.due_date = v || null)} /></Field>
        {/if}
        <p class={hintClass}>{$t(object?.type === 'body' ? 'weight.reminder-hint' : 'reminder.reading-hint')}</p>
      {:else if !input.schedule}
        <Field id="dd" label={$t('reminder.due-date')} error={errorFor('dd')}>
          <DateInput id="dd" bind:value={() => input.due_date ?? '', (v) => (input.due_date = v || null)} />
        </Field>
      {/if}
    {/if}

    {#if showCounter}
      <div class="grid grid-cols-2 gap-3">
        <Field id="dc" label={$t('reminder.due-counter')} unit={object?.counter_unit ?? null} error={errorFor('dc')}>
          <Input type="number" inputmode="numeric" min="0" bind:value={input.due_counter} />
        </Field>
        <Field id="rc" label={$t('reminder.repeat-counter')} unit={object?.counter_unit ?? null} error={errorFor('rc')}>
          <Input type="number" inputmode="numeric" min="1" bind:value={input.repeat_counter} />
        </Field>
      </div>
    {/if}

    <MoreDetails bind:open={moreOpen}>
      <Field id="no" label={$t('reminder.notes')}><Textarea bind:value={input.notes} /></Field>
    </MoreDetails>

    <FormActions {busy} error={formError} oncancel={() => back(`/objects/${oid}?tab=reminders`)} />
  </form>
  {#if editing}
    <section aria-labelledby="reminder-delete" class="mt-8 flex max-w-[40rem] flex-col gap-2 border-t border-border pt-4">
      <h2 id="reminder-delete" class={sectionHeadingClass}>{$t('nav.delete')}</h2>
      <Button variant="destructive" class="min-h-11 w-fit" disabled={busy} onclick={remove}>{$t('nav.delete')}</Button>
    </section>
  {/if}
</main>
