<script lang="ts">
  import { onMount } from 'svelte';
  import { api, fileUrl } from './api';
  import FilePicker from './FilePicker.svelte';
  import Icon from './Icon.svelte';
  import { t } from '../i18n';
  import { toInput } from './object-form';
  import type { Attachment, MemObject, ObjectInput } from './types';

  let { objectId, coverAttachmentId, onchanged }:
    { objectId: number; coverAttachmentId: number | null; onchanged?: () => void } = $props();
  let items = $state<Attachment[]>([]);

  async function load() { items = await api<Attachment[]>('GET', `/objects/${objectId}/attachments`); }
  onMount(load);

  async function remove(a: Attachment) {
    if (!confirm($t('nav.confirm-delete'))) return;
    await api('DELETE', `/attachments/${a.id}`);
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
    const o = await api<MemObject>('GET', `/objects/${objectId}`);
    const body: ObjectInput = { ...toInput(o), cover_attachment_id: cover };
    await api('PATCH', `/objects/${objectId}`, body);
    onchanged?.();
  }
</script>

<FilePicker {objectId} onuploaded={() => { load(); onchanged?.(); }} />

{#if items.length === 0}
  <p class="muted">{$t('docs.empty')}</p>
{:else}
  <div class="grid">
    {#each items as a (a.id)}
      <figure>
        {#if a.kind === 'photo'}
          <a href={fileUrl(a.file_id)} target="_blank" rel="noopener"><img class="thumb" src={fileUrl(a.file_id, true)} alt={a.caption || a.original_name} loading="lazy" /></a>
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
  figure { margin: 0; display: flex; flex-direction: column; gap: 4px; }
  figcaption { font-size: .8rem; display: flex; flex-direction: column; gap: 2px; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .small button { min-height: 32px; padding: 2px 6px; font-size: .75rem; }
  .danger-text { color: var(--danger); }
</style>
