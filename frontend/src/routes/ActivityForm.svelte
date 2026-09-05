<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import FilePicker from '../lib/FilePicker.svelte';
  import { api, fileUrl } from '../lib/api';
  import { go, back } from '../lib/router';
  import { centsToInput, counter as fmtCounter, fmtDate, parseMoney } from '../lib/format';
  import { emptyActivity, exifDate, toActivityInput, validateActivity } from '../lib/activity-form';
  import { locale, t } from '../i18n';
  import { CATEGORIES, type Activity, type Attachment, type MemObject, type ActivityInput } from '../lib/types';

  let { id, aid }: { id: string; aid?: string } = $props();
  const oid = $derived(Number(id));
  const editing = $derived(aid !== undefined);

  let object = $state<MemObject | null>(null);
  let input = $state<ActivityInput>(emptyActivity());
  let costText = $state('');
  let counterText = $state('');
  let attachments = $state<Attachment[]>([]);
  let saved = $state<Activity | null>(null);
  let error = $state('');
  let busy = $state(false);

  const lastCounter = $derived(object?.stats.current_counter ?? null);
  const counterWarn = $derived(
    counterText !== '' && lastCounter !== null && Number(counterText) < lastCounter,
  );
  const photoDate = $derived(attachments.map(exifDate).find((d) => d !== null) ?? null);

  onMount(async () => {
    object = await api<MemObject>('GET', `/objects/${oid}`);
    if (aid) {
      const a = await api<Activity>('GET', `/activities/${aid}`);
      saved = a;
      input = toActivityInput(a);
      costText = centsToInput(a.cost_cents);
      counterText = a.counter_value === null ? '' : String(a.counter_value);
      attachments = a.attachments;
    } else if (object.stats.current_counter !== null) {
      counterText = String(object.stats.current_counter);
    }
  });

  /** Files need an activity row to hang on, so save the draft first. */
  async function ensureSaved(): Promise<Activity> {
    if (saved) return saved;
    const body = buildInput();
    const bad = validateActivity(body);
    if (bad) throw new Error($t(bad));
    saved = await api<Activity>('POST', `/objects/${oid}/activities`, body);
    return saved;
  }

  function buildInput(): ActivityInput {
    return {
      ...input,
      cost_cents: parseMoney(costText),
      counter_value: counterText.trim() === '' ? null : Number(counterText),
    };
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const body = buildInput();
    const bad = validateActivity(body);
    if (bad) { error = $t(bad); return; }
    busy = true; error = '';
    try {
      if (saved) await api('PATCH', `/activities/${saved.id}`, body);
      else await api('POST', `/objects/${oid}/activities`, body);
      go(`/objects/${oid}`, true);
    } catch (err) { error = (err as Error).message; } finally { busy = false; }
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
    <div class="field"><label for="ti">{$t('activity.title')}</label><input id="ti" bind:value={input.title} required /></div>
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
    {:else}
      <button type="button" class="ghost pickerlike" onclick={async () => { try { await ensureSaved(); } catch (e) { error = (e as Error).message; } }}>
        + {$t('activity.add-files')}
      </button>
    {/if}

    {#if error}<p class="error">{error}</p>{/if}
    <div class="row actions">
      <button type="button" class="ghost" onclick={() => back(`/objects/${oid}`)}>{$t('nav.cancel')}</button>
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
