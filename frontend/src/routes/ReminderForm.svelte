<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import { api } from '../lib/api';
  import { go, back } from '../lib/router';
  import { t } from '../i18n';
  import { emptyReminder, toReminderInput, validateReminder } from '../lib/reminder-form';
  import type { MemObject, Reminder, ReminderInput } from '../lib/types';

  let { id, rid }: { id: string; rid?: string } = $props();
  const oid = $derived(Number(id));
  const editing = $derived(rid !== undefined);
  let object = $state<MemObject | null>(null);
  let input = $state<ReminderInput>(emptyReminder());
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    object = await api<MemObject>('GET', `/objects/${oid}`);
    if (rid) input = toReminderInput(await api<Reminder>('GET', `/reminders/${rid}`));
  });

  function normalized(): ReminderInput {
    return {
      ...input,
      due_date: input.due_date || null,
      due_counter: input.due_counter === null || String(input.due_counter) === '' ? null : Number(input.due_counter),
      repeat_months: input.repeat_months === null || String(input.repeat_months) === '' ? null : Number(input.repeat_months),
      repeat_counter: input.repeat_counter === null || String(input.repeat_counter) === '' ? null : Number(input.repeat_counter),
    };
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const body = normalized();
    const bad = validateReminder(body);
    if (bad) { error = $t(bad); return; }
    busy = true; error = '';
    try {
      if (rid) await api('PATCH', `/reminders/${rid}`, body);
      else await api('POST', `/objects/${oid}/reminders`, body);
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
    <div class="field"><label for="ti">{$t('reminder.title')}</label><input id="ti" bind:value={input.title} required /></div>
    <div class="row">
      <div class="field"><label for="dd">{$t('reminder.due-date')}</label><input id="dd" type="date" bind:value={input.due_date} /></div>
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
</style>
