<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import { api, createQueued, isRejection } from '../lib/api';
  import { getCachedObject } from '../lib/object-cache';
  import { back, go } from '../lib/router';
  import { counter, fmtDate, todayIso } from '../lib/format';
  import { readingActivity, readingWarning, type ReadingWarning } from '../lib/reading';
  import { locale, t } from '../i18n';
  import type { Activity, Insights, MemObject } from '../lib/types';

  let { id }: { id: string } = $props();
  const oid = $derived(Number(id));

  let object = $state<MemObject | null>(null);
  let valueText = $state('');
  let date = $state(todayIso());
  let lastDate = $state<string | null>(null);
  let rate = $state<number | null>(null);
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
      else { error = (e as Error).message; return; }
    }
    if (object.stats.current_counter !== null) valueText = String(object.stats.current_counter);
    // Both lookups only sharpen the plausibility check; the form is complete without them.
    try {
      const page = await api<{ items: Activity[] } | Activity[]>('GET', `/objects/${oid}/activities?limit=20`);
      const list = Array.isArray(page) ? page : page.items;
      lastDate = list.filter((a) => a.counter_value !== null).map((a) => a.date).sort().at(-1) ?? null;
    } catch { /* offline or refused: no date, no rate check */ }
    try { rate = (await api<Insights>('GET', `/objects/${oid}/insights`)).counter_per_day_milli; }
    catch { /* same */ }
  });

  const value = $derived(String(valueText).trim() === '' ? NaN : Number(valueText));
  const warning = $derived(
    object && Number.isFinite(value)
      ? readingWarning(value, date, { lastCounter: object.stats.current_counter, lastDate, ratePerDayMilli: rate })
      : null,
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
      error = $t((err as Error).message);
    } finally { busy = false; }
  }
</script>

<main>
  <TopBar title={$t('reading.title')} backTo={`/objects/${oid}`} />
  {#if object}
    <form onsubmit={submit}>
      <p class="muted">{object.name}</p>
      <div class="field">
        <label for="rv">{$t('reading.value', { unit: object.counter_unit ?? '' })}</label>
        <!-- Focused straight away: this form has one job, and it is usually opened from a
             notification with the odometer in front of the person. -->
        <!-- svelte-ignore a11y_autofocus -->
        <input id="rv" class="tnum big" type="number" inputmode="numeric" min="0" step="1" bind:value={valueText} autofocus required />
        {#if object.stats.current_counter !== null && lastDate}
          <span class="hint tnum">{$t('reading.last', { counter: counter(object.stats.current_counter, object.counter_unit, $locale), date: fmtDate(lastDate, $locale) })}</span>
        {/if}
      </div>
      <div class="field"><label for="rd">{$t('activity.date')}</label><input id="rd" type="date" bind:value={date} max={todayIso()} required /></div>
      {#if acknowledged && warning === acknowledged.warning}
        <p class="warn" role="alert">
          {warning === 'lower'
            ? $t('reading.warn-lower', { last: counter(object.stats.current_counter, object.counter_unit, $locale) })
            : $t('reading.warn-implausible', { date: fmtDate(lastDate, $locale) })}
        </p>
      {/if}
      {#if error}<p class="error">{error}</p>{/if}
      <div class="row actions">
        <button type="button" class="ghost" onclick={() => back(`/objects/${oid}`)}>{$t('nav.cancel')}</button>
        <button class="primary" disabled={busy}>{$t('nav.save')}</button>
      </div>
    </form>
  {:else if error}
    <p class="error">{error}</p>
  {:else}
    <p class="muted">{$t('nav.loading')}</p>
  {/if}
</main>

<style>
  .big { font-size: var(--text-data); }
  .actions { margin-top: var(--space-2); }
</style>
