<script lang="ts">
  import { errorMessage } from './api-error';
  import { onDestroy } from 'svelte';
  import { uploadQueued } from './api';
  import { newOpId } from './outbox';
  import { hashToNegativeId } from './activity-form';
  import { shrinkImage } from './downscale';
  import { t } from '../i18n';
  import Icon from './Icon.svelte';
  import { Button } from '$lib/components/ui/button/index.js';
  import { errorClass } from '$lib/components/ui/field/classes.js';
  import type { Attachment, Kind } from './types';

  let { objectId, activityId = null, onuploaded }: { objectId: number; activityId?: number | null; onuploaded: (a: Attachment) => void } = $props();
  let busy = $state(false);
  let error = $state('');
  let el: HTMLInputElement;
  let cam: HTMLInputElement;
  /** Object URLs handed out as `previewUrl` below, kept only so they can be revoked when this
   *  picker goes away -- `URL.createObjectURL` pins the underlying blob in memory until
   *  explicitly revoked, and nothing else in the app ever calls `revokeObjectURL` for these. */
  const objectUrls: string[] = [];
  onDestroy(() => { for (const u of objectUrls) URL.revokeObjectURL(u); });

  /** A negative placeholder id for an attachment that only reached the outbox, so it can sit
   *  in an `Attachment[]`-keyed list without colliding with a real (always positive) one.
   *  `newOpId`, not `crypto.randomUUID`: the latter does not exist on a plain-http origin
   *  (see ./outbox.ts), where it made every offline photo pick throw. */
  function pendingAttachmentId(): number {
    return hashToNegativeId(newOpId());
  }

  async function send(files: FileList | null) {
    if (!files || files.length === 0) return;
    busy = true; error = '';
    try {
      for (const picked of Array.from(files)) {
        const kind: Kind = picked.type.startsWith('image/') ? 'photo' : 'document';
        // A full-size phone photo was most of what Save waited on; see ./downscale.ts. Before
        // the outbox, so a queued photo is the small one too.
        const f = await shrinkImage(picked);
        // `uploadQueued` mints its own `client_op_id` per call (see ./api.ts), so each file in
        // a multi-file selection still gets its own -- unchanged from before this switched
        // from the plain `upload()`.
        const result = await uploadQueued<Attachment>(`/objects/${objectId}/attachments`, f, f.name, activityId ?? undefined);
        if (result) {
          onuploaded(result);
        } else {
          // Queued rather than sent: the photo must still appear to the user right away -- a
          // photo that vanishes because there was no signal at the fuel pump is exactly the
          // failure this feature exists to prevent. There is no server `file_id` yet, so the
          // preview renders straight from the picked File; `pending` tells the caller's list
          // to dim it and tag it, exactly like a queued activity in Timeline.svelte.
          let previewUrl: string | undefined;
          if (kind === 'photo') {
            previewUrl = URL.createObjectURL(f);
            objectUrls.push(previewUrl);
          }
          onuploaded({
            id: pendingAttachmentId(), object_id: objectId, activity_id: activityId ?? null,
            file_id: 0, kind, caption: '', created_at: new Date().toISOString(),
            original_name: f.name, mime: f.type, size: f.size, width: null, height: null, taken_at: null,
            pending: true, previewUrl,
          });
        }
      }
    // Save does not wait for this: the form may be gone by now, with the inputs unbound, while
    // the upload itself goes on to the activity it was picked for.
    } catch (e) { error = errorMessage(e, $t); } finally { busy = false; if (el) el.value = ''; if (cam) cam.value = ''; }
  }
</script>

<div data-slot="file-picker" class="flex flex-col gap-2">
  <input bind:this={el} type="file" class="hidden" multiple accept="image/*,application/pdf,.txt,.md,.doc,.docx,.xls,.xlsx"
         onchange={(e) => send((e.currentTarget as HTMLInputElement).files)} />
  <!-- `capture` cannot live on the input above: on mobile it suppresses picking an existing file. -->
  <input bind:this={cam} type="file" class="hidden" accept="image/*" capture="environment"
         onchange={(e) => send((e.currentTarget as HTMLInputElement).files)} />
  <div class="grid grid-cols-2 gap-2">
    <Button variant="outline" class="min-h-12 border-dashed whitespace-normal" disabled={busy} onclick={() => el.click()}>
      {busy ? $t('activity.uploading') : `+ ${$t('activity.add-files')}`}
    </Button>
    <Button variant="outline" class="min-h-12 border-dashed whitespace-normal" disabled={busy} onclick={() => cam.click()}><Icon name="camera" size={18} /> {$t('activity.take-photo')}</Button>
  </div>
  {#if error}<p class={errorClass} aria-live="polite">{error}</p>{/if}
</div>
