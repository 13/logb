<script lang="ts">
  import { errorMessage } from '../lib/api-error';
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import DateInput from '../lib/DateInput.svelte';
  import { api, createQueued, isRejection } from '../lib/api';
  import { getCachedObject } from '../lib/object-cache';
  import { back, go } from '../lib/router';
  import { counter, fmtDate, todayIso } from '../lib/format';
  import { dateFormat } from '../stores/date-format';
  import { readingActivity, readingWarning, type ReadingWarning } from '../lib/reading';
  import { locale, t } from '../i18n';
  import FormActions from '../lib/FormActions.svelte';
  import { Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { errorClass, hintClass, warnClass } from '$lib/components/ui/field/classes.js';
  import type { MemObject } from '../lib/types';

  let { id }: { id: string } = $props();
  const oid = $derived(Number(id));

  let object = $state<MemObject | null>(null);
  let valueText = $state('');
  let date = $state(todayIso());
  let lastDate = $state<string | null>(null);
  let rate = $state<number | null>(null);
  let valueInput = $state<HTMLInputElement | null>(null);
  let error = $state('');
  let busy = $state(false);
  /** The warning the user has already been shown for exactly this value and date. Saving again
   *  unchanged is the confirmation; editing either field asks again. */
  let acknowledged = $state<{ warning: ReadingWarning; key: string } | null>(null);

  onMount(async () => {
    try {
      object = await api<MemObject>('GET', `/objects/${oid}`);
    } catch (e) {
      // Offline, this form still works from the cached object: the reading goes to the outbox.
      const cached = isRejection(e) ? undefined : getCachedObject(oid);
      if (cached) object = cached;
      else { error = errorMessage(e, $t); return; }
    }
    if (object.type === 'body') { go(`/objects/${oid}/activities/new?category=weight`, true); return; }
    if (object.stats.current_counter !== null) valueText = String(object.stats.current_counter);
    lastDate = object.stats.last_reading_date;
    // The rate only sharpens the plausibility check; the form is complete without it.
    try { rate = (await api<{ counter_per_day_milli: number | null }>('GET', `/objects/${oid}/usage`)).counter_per_day_milli; }
    catch { /* offline or refused: no rate check */ }
  });

  // Focused straight away: this form has one job, and it is usually opened from a notification
  // with the odometer in front of the person. (`autofocus` on a component's input is only an
  // attribute by the time it is inserted, and the browser ignores that.)
  $effect(() => { valueInput?.focus(); });

  const value = $derived(String(valueText).trim() === '' ? NaN : Number(valueText));
  const warning = $derived(
    object && Number.isFinite(value)
      ? readingWarning(value, date, { lastCounter: object.stats.current_counter, lastDate, ratePerDayMilli: rate })
      : null,
  );

  const lastHint = $derived(
    object && object.stats.current_counter !== null && lastDate
      ? $t('reading.last', { counter: counter(object.stats.current_counter, object.counter_unit, $locale), date: fmtDate(lastDate, $dateFormat) })
      : '',
  );

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!object || !Number.isInteger(value) || value < 0) { error = $t('reading.value', { unit: object?.counter_unit ?? '' }); return; }
    const key = `${value}|${date}`;
    if (warning && (acknowledged?.warning !== warning || acknowledged.key !== key)) {
      acknowledged = { warning, key };
      return;
    }
    busy = true; error = '';
    try {
      // `createQueued`, as the activity form uses: a reading taken in an underground car park
      // waits in the outbox and clears the reminder once it lands.
      await createQueued(`/objects/${oid}/activities`, readingActivity(value, date, $t('reading.entry-title')));
      go(`/objects/${oid}`, true);
    } catch (err) {
      error = errorMessage(err, $t);
    } finally { busy = false; }
  }
</script>

<main>
  <TopBar title={$t('reading.title')} subtitle={object?.name ?? null} backTo={`/objects/${oid}`} />
  {#if object}
    <form onsubmit={submit} class="m-0 flex w-full max-w-[40rem] flex-col gap-5">
      <Field id="rv" label={$t('reading.value', { unit: object.counter_unit ?? '' })} hint={lastHint}>
        <Input type="number" inputmode="numeric" min="0" step="1" bind:ref={valueInput} bind:value={valueText} required class="h-14 text-2xl tabular-nums" />
      </Field>
      <Field id="rd" label={$t('activity.date')}><DateInput id="rd" bind:value={date} max={todayIso()} required /></Field>
      <!-- An alert, not the field's quiet warning: it is the reason Save did not go through, and
           saving again unchanged is the confirmation. -->
      {#if acknowledged && warning === acknowledged.warning}
        <p role="alert" class={warnClass}>
          {warning === 'lower'
            ? $t('reading.warn-lower', { last: counter(object.stats.current_counter, object.counter_unit, $locale) })
            : $t('reading.warn-implausible', { date: fmtDate(lastDate, $dateFormat) })}
        </p>
      {/if}
      <FormActions {busy} {error} oncancel={() => back(`/objects/${oid}`)} />
    </form>
  {:else if error}
    <p role="alert" class={errorClass}>{error}</p>
  {:else}
    <p class={hintClass}>{$t('nav.loading')}</p>
  {/if}
</main>
