<script lang="ts">
  import { onMount } from 'svelte';
  import { api, fileUrl } from './api';
  import FilePicker from './FilePicker.svelte';
  import { t } from '../i18n';
  import type { Attachment } from './types';

  let { objectId, onchanged }: { objectId: number; onchanged?: () => void } = $props();
  let items = $state<Attachment[]>([]);

  async function load() { items = await api<Attachment[]>('GET', `/objects/${objectId}/attachments`); }
  onMount(load);

  async function remove(a: Attachment) {
    if (!confirm($t('nav.confirm-delete'))) return;
    await api('DELETE', `/attachments/${a.id}`);
    await load();
    onchanged?.();
  }

  async function setCover(a: Attachment) {
    const o = await api<{ name: string; category: string; counter_unit: string | null; description: string; purchase_date: string | null; purchase_price_cents: number | null; archived_at: string | null }>('GET', `/objects/${objectId}`);
    await api('PATCH', `/objects/${objectId}`, {
      name: o.name, category: o.category, counter_unit: o.counter_unit, description: o.description,
      purchase_date: o.purchase_date, purchase_price_cents: o.purchase_price_cents,
      archived: o.archived_at !== null, cover_attachment_id: a.id,
    });
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
          <a class="doc-icon" href={fileUrl(a.file_id)} target="_blank" rel="noopener">📄</a>
        {/if}
        <figcaption>
          <span class="name">{a.caption || a.original_name}</span>
          <span class="row small">
            {#if a.kind === 'photo'}<button class="ghost" onclick={() => setCover(a)}>{$t('object.set-cover')}</button>{/if}
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
