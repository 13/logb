<script lang="ts">
  import { errorMessage } from './api-error';
  import { untrack } from 'svelte';
  import { createSeq } from './seq-guard';
  import { api, fileUrl } from './api';
  import FilePicker from './FilePicker.svelte';
  import Icon from './Icon.svelte';
  import ImageUp from '@lucide/svelte/icons/image-up';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import { Button } from '$lib/components/ui/button/index.js';
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
  const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring';
</script>

<FilePicker {objectId} onuploaded={() => { load(); onchanged?.(); }} />

{#if error}<p class="error" role="alert">{error}</p>{/if}

{#if !loaded}
  <!-- Nothing: the request is still out. -->
{:else if items.length === 0}
  <!-- The picker sits right above this, so the words only have to say what is worth putting
       into it. Not after a failed load: the error above already says why there is no list. -->
  {#if !error}
    <div class="flex flex-col items-center gap-3 px-4 py-10 text-center">
      <span class="text-muted-foreground opacity-40"><Icon name="document" size={40} /></span>
      <p class="m-0 max-w-[34ch] text-sm text-muted-foreground">{$t('docs.empty')}</p>
    </div>
  {/if}
{:else}
  <ul role="list" class="m-0 mt-3 grid list-none grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-3 p-0">
    {#each items as a (a.id)}
      <li data-testid="document" class="flex min-w-0 flex-col gap-1">
        {#if a.kind === 'photo'}
          <a href={fileUrl(a.file_id)} target="_blank" rel="noopener" class={`block rounded-md ${focus}`}>
            <img class="block aspect-square w-full rounded-md bg-muted object-cover" src={fileUrl(a.file_id, true)} alt={a.caption || a.original_name} loading="lazy" decoding="async" />
          </a>
        {:else}
          <a href={fileUrl(a.file_id)} target="_blank" rel="noopener" aria-label={a.caption || a.original_name}
             class={`grid aspect-square w-full place-items-center rounded-md bg-muted text-muted-foreground ${focus}`}><Icon name="document" size={32} /></a>
        {/if}
        <span class="truncate text-xs text-foreground">{a.caption || a.original_name}</span>
        <div class="flex gap-1">
          {#if a.kind === 'photo'}
            {@const isCover = a.id === coverAttachmentId}
            {@const label = $t('object.cover-photo')}
            <Button variant="ghost" class="size-11" aria-label={label} title={label} aria-pressed={isCover}
                    onclick={() => setCover(isCover ? null : a.id)}>
              <ImageUp class={isCover ? 'text-brand-ink' : ''} fill={isCover ? 'currentColor' : 'none'} fill-opacity={isCover ? 0.25 : 0} />
            </Button>
          {/if}
          <Button variant="ghost" class="size-11 text-destructive hover:bg-destructive/10 hover:text-destructive" aria-label={$t('doc.delete-named', { name: a.caption || a.original_name })} title={$t('nav.delete')} onclick={() => remove(a)}>
            <Trash2 />
          </Button>
        </div>
      </li>
    {/each}
  </ul>
{/if}
