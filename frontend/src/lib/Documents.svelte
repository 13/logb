<script lang="ts">
  import { errorMessage } from './api-error';
  import { untrack } from 'svelte';
  import { createSeq } from './seq-guard';
  import { api, fileUrl } from './api';
  import FilePicker from './FilePicker.svelte';
  import Icon from './Icon.svelte';
  import { t } from '../i18n';
  import { toInput } from './object-form';
  import type { Attachment, MemObject, ObjectInput } from './types';

  let { objectId, coverAttachmentId, onchanged }:
    { objectId: number; coverAttachmentId: number | null; onchanged?: () => void } = $props();
  let items = $state<Attachment[]>([]);
  /** Whether the answer is known. `items` starts empty because it has to start as something,
   *  and drawing the empty state from that means announcing "nothing here" before anyone has
   *  looked -- a full icon-and-sentence block that flashes away when the list arrives. It stays
   *  true once the first answer is in: a later reload is a refresh of a known list, not another
   *  question about whether there is one. */
  let loaded = $state(false);
  /** A failed load says so, instead of falling through to "no documents yet" -- which would
   *  claim an answer nobody got. Also carries a failed delete or cover change. */
  let error = $state('');

  /// ObjectDetail reuses this instance when it moves to another object (only `objectId`
  /// changes), so a list requested for the object just left must not land on the new one.
  const loadSeq = createSeq();

  async function load() {
    const token = loadSeq.next();
    const target = objectId;
    try {
      const rows = await api<Attachment[]>('GET', `/objects/${target}/attachments`);
      if (!loadSeq.current(token)) return;
      items = rows;
      error = '';
    } catch (e) {
      if (!loadSeq.current(token)) return;
      error = errorMessage(e, $t);
    } finally {
      if (loadSeq.current(token)) loaded = true;
    }
  }

  // Not `onMount`: see `loadSeq`. A new object starts unknown, not with the last one's files.
  $effect(() => {
    objectId;
    untrack(() => { items = []; loaded = false; error = ''; void load(); });
  });

  async function remove(a: Attachment) {
    if (!confirm($t('nav.confirm-delete'))) return;
    try {
      await api('DELETE', `/attachments/${a.id}`);
    } catch (e) {
      error = errorMessage(e, $t);
      return;
    }
    await load();
    onchanged?.();
  }

  /** `null` clears the cover; the API tells "omitted" (keep) from an explicit null (clear).
   *
   *  PATCH on an object is a full replace for every field except the cover, so this
   *  read-modify-write has to send the object back whole: any field left out is deserialized
   *  as `None` and written as NULL. It builds the body through `toInput` -- the same helper
   *  ObjectForm uses -- and types it as `ObjectInput`, so a column added to the object in
   *  future is a COMPILE error here rather than a field this function silently wipes.
   *  Hand-listing the fields inline is how setting a cover photo came to clear `fuel_unit`,
   *  quietly moving an e-bike's insights back from kWh to litres. */
  async function setCover(cover: number | null) {
    try {
      const o = await api<MemObject>('GET', `/objects/${objectId}`);
      const body: ObjectInput = { ...toInput(o), cover_attachment_id: cover };
      await api('PATCH', `/objects/${objectId}`, body);
      error = '';
    } catch (e) {
      error = errorMessage(e, $t);
      return;
    }
    onchanged?.();
  }
</script>

<FilePicker {objectId} onuploaded={() => { load(); onchanged?.(); }} />

{#if error}<p class="error" role="alert">{error}</p>{/if}

{#if !loaded}
  <!-- Nothing: the request is still out. -->
{:else if items.length === 0}
  <!-- The picker sits right above this, so the words only have to say what is worth putting
       into it. Not after a failed load: the error above already says why there is no list. -->
  {#if !error}
    <div class="empty">
      <span class="empty-icon"><Icon name="document" size={40} /></span>
      <p>{$t('docs.empty')}</p>
    </div>
  {/if}
{:else}
  <div class="thumb-grid">
    {#each items as a (a.id)}
      <figure>
        {#if a.kind === 'photo'}
          <a href={fileUrl(a.file_id)} target="_blank" rel="noopener"><img class="thumb" src={fileUrl(a.file_id, true)} alt={a.caption || a.original_name} loading="lazy" decoding="async" /></a>
        {:else}
          <a class="doc-icon" href={fileUrl(a.file_id)} target="_blank" rel="noopener" aria-label={a.caption || a.original_name}><Icon name="document" size={32} /></a>
        {/if}
        <figcaption>
          <span class="name">{a.caption || a.original_name}</span>
          <span class="row small">
            {#if a.kind === 'photo'}
              {#if a.id === coverAttachmentId}
                <button class="ghost" onclick={() => setCover(null)}>{$t('object.clear-cover')}</button>
              {:else}
                <button class="ghost" onclick={() => setCover(a.id)}>{$t('object.set-cover')}</button>
              {/if}
            {/if}
            <button class="ghost danger-text" onclick={() => remove(a)}>{$t('nav.delete')}</button>
          </span>
        </figcaption>
      </figure>
    {/each}
  </div>
{/if}

<style>
  figure { margin: 0; display: flex; flex-direction: column; gap: var(--space-1); }
  figcaption { font-size: var(--text-xs); display: flex; flex-direction: column; gap: var(--space-1); }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .small button { min-height: 32px; padding: 2px var(--space-2); font-size: var(--text-xs); }
  .danger-text { color: var(--danger); }
</style>
