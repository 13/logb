<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import DateInput from '../lib/DateInput.svelte';
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

  onMount(async () => {
    object = await api<MemObject>('GET', `/objects/${oid}`);
    if (rid) input = toReminderInput(await api<Reminder>('GET', `/reminders/${rid}`));
    else if (presetKind === 'reading' && (object.counter_unit || object.type === 'body')) input = readingReminder($t(object?.type === 'body' ? 'weight.reminder' : 'reading.reminder-title'));
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
    {#if input.kind === 'reading'}
      <div class="row">
        <div class="field"><label for="en">{$t('reminder.every')}</label><input id="en" type="number" inputmode="numeric" min="1" max="60" bind:value={input.every_n} /></div>
        <div class="field">
          <label for="eu">{$t('reminder.every-unit')}</label>
          <select id="eu" bind:value={input.every_unit}>
            <option value="week">{$t('reminder.unit-week')}</option>
            <option value="month">{$t('reminder.unit-month')}</option>
          </select>
        </div>
      </div>
      <div class="field"><label for="st">{$t('reminder.starts')}</label><DateInput id="st" bind:value={() => input.due_date ?? '', (v) => (input.due_date = v || null)} /></div>
      <p class="hint">{$t(object?.type === 'body' ? 'weight.reminder-hint' : 'reminder.reading-hint')}</p>
    {:else}
      <div class="row">
        <div class="field"><label for="dd">{$t('reminder.due-date')}</label><DateInput id="dd" bind:value={() => input.due_date ?? '', (v) => (input.due_date = v || null)} /></div>
        {#if object?.counter_unit}
          <div class="field"><label for="dc">{$t('reminder.due-counter')} ({object.counter_unit})</label><input id="dc" type="number" min="0" bind:value={input.due_counter} /></div>
        {/if}
      </div>
      <div class="row">
        <div class="field"><label for="rm">{$t('reminder.repeat-months')}</label><input id="rm" type="number" min="1" bind:value={input.repeat_months} /></div>
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
