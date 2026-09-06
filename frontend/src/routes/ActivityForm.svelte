<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import FilePicker from '../lib/FilePicker.svelte';
  import { api, createQueued, fileUrl } from '../lib/api';
  import { go, back } from '../lib/router';
  import { centsToInput, counter as fmtCounter, fmtDate, parseMoney, parseQuantity } from '../lib/format';
  import { emptyActivity, exifDate, suggestionsFor, toActivityInput, validateActivity } from '../lib/activity-form';
  import { locale, t } from '../i18n';
  import { CATEGORIES, type Activity, type Attachment, type MemObject, type ActivityInput, type TitleSuggestion } from '../lib/types';

  let { id, aid }: { id: string; aid?: string } = $props();
  const oid = $derived(Number(id));
  const editing = $derived(aid !== undefined);

  let object = $state<MemObject | null>(null);
  let input = $state<ActivityInput>(emptyActivity());
  let costText = $state('');
  let counterText = $state('');
  let quantityText = $state('');
  let attachments = $state<Attachment[]>([]);
  let saved = $state<Activity | null>(null);
  let error = $state('');
  let busy = $state(false);
  let allSuggestions = $state<TitleSuggestion[]>([]);
  const suggestions = $derived(suggestionsFor(allSuggestions, input.category));
  /** True when `saved` exists only because the user attached a file, never because they saved. */
  let autoDraft = $state(false);
  /** False only while editing an existing activity whose GET hasn't resolved yet — blocks the
   *  "add files" affordance so it can't spuriously POST a new row before we know one already exists. */
  let ready = $state(untrack(() => aid === undefined));

  const lastCounter = $derived(object?.stats.current_counter ?? null);
  const counterWarn = $derived(
    counterText !== '' && lastCounter !== null && Number(counterText) < lastCounter,
  );
  const photoDate = $derived(attachments.map(exifDate).find((d) => d !== null) ?? null);

  onMount(async () => {
    object = await api<MemObject>('GET', `/objects/${oid}`);
    allSuggestions = await api<TitleSuggestion[]>('GET', `/objects/${oid}/recent-titles`);
    if (aid) {
      const a = await api<Activity>('GET', `/activities/${aid}`);
      saved = a;
      autoDraft = false; // this row predates the form; never let a stray click earlier mark it disposable
      input = toActivityInput(a);
      costText = centsToInput(a.cost_cents);
      counterText = a.counter_value === null ? '' : String(a.counter_value);
      quantityText = a.quantity_milli === null ? '' : String(a.quantity_milli / 1000);
      attachments = a.attachments;
      ready = true;
    } else if (object.stats.current_counter !== null) {
      counterText = String(object.stats.current_counter);
    }
  });

  /** Files need an activity row to hang on, so save the draft first. */
  async function ensureSaved(): Promise<Activity> {
    if (saved) return saved;
    if (!ready) throw new Error('not loaded yet');
    const body = buildInput();
    const bad = validateActivity(body);
    if (bad) throw new Error($t(bad));
    saved = await api<Activity>('POST', `/objects/${oid}/activities`, body);
    autoDraft = true;
    return saved;
  }

  function buildInput(): ActivityInput {
    return {
      ...input,
      cost_cents: parseMoney(costText),
      // `counterText` is bound to a number input, so Svelte hands back a number, not a string.
      counter_value: String(counterText).trim() === '' ? null : Number(counterText),
      // Same comma/dot handling as parseMoney, so this field and cost agree on what's valid input.
      quantity_milli: parseQuantity(quantityText),
    };
  }

  /** Prefill from a past entry. The user still reviews and saves; nothing is written here. */
  function repeat(s: TitleSuggestion) {
    input.title = s.title;
    input.category = s.category;
    if (s.last_cost_cents !== null) costText = centsToInput(s.last_cost_cents);
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const body = buildInput();
    const bad = validateActivity(body);
    if (bad) { error = $t(bad); return; }
    busy = true; error = '';
    try {
      if (saved) {
        await api('PATCH', `/activities/${saved.id}`, body);
      } else {
        // null means "queued, not sent": the row exists locally and will be replayed.
        await createQueued<Activity>(`/objects/${oid}/activities`, body as unknown as Record<string, unknown>);
      }
      autoDraft = false;
      go(`/objects/${oid}`, true);
    } catch (err) {
      // `createQueued` throws an i18n key (rather than a message) when the write reached
      // neither the server nor the local outbox queue, so the entry is honestly reported as
      // lost instead of navigating away as though it had been saved. `$t` on any other
      // (plain-English, server-supplied) message just returns it unchanged.
      error = $t((err as Error).message);
    } finally { busy = false; }
  }

  /** Cancel throws the auto-created draft away; keeping it would leave a stray timeline entry. */
  async function cancel() {
    if (autoDraft && saved) {
      if (attachments.length > 0 && !confirm($t('activity.discard-draft'))) return;
      try { await api('DELETE', `/activities/${saved.id}`); } catch { /* leaving it is better than blocking the exit */ }
    }
    back(`/objects/${oid}`);
  }

  async function remove() {
    if (!saved || !confirm($t('nav.confirm-delete'))) return;
    await api('DELETE', `/activities/${saved.id}`);
    go(`/objects/${oid}`, true);
  }
</script>

<main>
  <TopBar title={editing ? $t('activity.edit') : $t('activity.new')} backTo={`/objects/${oid}`} />
  <form onsubmit={submit}>
    <div class="row">
      <div class="field"><label for="d">{$t('activity.date')}</label><input id="d" type="date" bind:value={input.date} required /></div>
      <div class="field">
        <label for="c">{$t('activity.category')}</label>
        <select id="c" bind:value={input.category}>
          {#each CATEGORIES as c}<option value={c}>{$t(`cat.${c}`)}</option>{/each}
        </select>
      </div>
    </div>
    {#if photoDate && photoDate !== input.date}
      <button type="button" class="ghost hintbtn" onclick={() => (input.date = photoDate)}>{$t('activity.use-exif-date', { date: fmtDate(photoDate, $locale) })}</button>
    {/if}
    {#if !editing && suggestions.length > 0}
      <div class="chips">
        {#each suggestions.slice(0, 3) as s (s.title + s.category)}
          <button type="button" class="chip" onclick={() => repeat(s)}>{$t('activity.repeat')}: {s.title}</button>
        {/each}
      </div>
    {/if}
    <div class="field">
      <label for="ti">{$t('activity.title')}</label>
      <input id="ti" list="titles" bind:value={input.title} required />
      <datalist id="titles">
        {#each suggestions as s (s.title + s.category)}<option value={s.title}></option>{/each}
      </datalist>
    </div>
    <div class="row">
      {#if object?.counter_unit}
        <div class="field">
          <label for="cv">{$t('activity.counter')} ({object.counter_unit})</label>
          <input id="cv" type="number" inputmode="numeric" min="0" bind:value={counterText} />
          {#if counterWarn}<span class="warn">{$t('activity.counter-warn', { last: fmtCounter(lastCounter, object.counter_unit, $locale) })}</span>{/if}
        </div>
      {/if}
      <div class="field"><label for="co">{$t('activity.cost')}</label><input id="co" type="text" inputmode="decimal" bind:value={costText} /></div>
    </div>
    {#if input.category === 'fuel' && object?.counter_unit}
      <div class="field">
        <label for="qt">{$t('activity.quantity')} ({object.fuel_unit ?? (object.counter_unit === 'mi' ? 'gal' : 'l')})</label>
        <input id="qt" type="text" inputmode="decimal" bind:value={quantityText} />
      </div>
    {/if}
    <div class="field"><label for="no">{$t('activity.notes')}</label><textarea id="no" bind:value={input.notes}></textarea></div>

    <h2>{$t('activity.photos')}</h2>
    {#if attachments.length > 0}
      <div class="thumb-strip">
        {#each attachments as a (a.id)}
          {#if a.kind === 'photo'}<img src={fileUrl(a.file_id, true)} alt="" />{:else}<span class="doc-chip">📄</span>{/if}
        {/each}
      </div>
    {/if}
    {#if saved}
      <FilePicker objectId={oid} activityId={saved.id} onuploaded={(a) => (attachments = [...attachments, a])} />
    {:else if ready}
      <button type="button" class="ghost pickerlike" onclick={async () => { try { await ensureSaved(); } catch (e) { error = (e as Error).message; } }}>
        + {$t('activity.add-files')}
      </button>
    {/if}

    {#if error}<p class="error">{error}</p>{/if}
    <div class="row actions">
      <button type="button" class="ghost" onclick={cancel}>{$t('nav.cancel')}</button>
      <button class="primary" disabled={busy}>{$t('nav.save')}</button>
    </div>
  </form>
  {#if editing}
    <button class="danger" onclick={remove}>{$t('nav.delete')}</button>
  {/if}
</main>

<style>
  .hintbtn { font-size: .85rem; color: var(--accent); padding: 4px 0; text-align: left; }
  .pickerlike { border: 1px dashed var(--border); width: 100%; }
  .doc-chip { display: grid; place-items: center; width: 64px; height: 64px; background: var(--surface-2); border-radius: 6px; }
  .actions { margin-top: 8px; }
</style>
