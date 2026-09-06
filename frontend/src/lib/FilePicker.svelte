<script lang="ts">
  import { onDestroy } from 'svelte';
  import { uploadQueued } from './api';
  import { t } from '../i18n';
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
   *  in an `Attachment[]`-keyed list without colliding with a real (always positive) one --
   *  same scheme as `pendingId` in ObjectDetail.svelte. */
  function pendingAttachmentId(): number {
    const s = crypto.randomUUID();
    let h = 0;
    for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
    return -(Math.abs(h) || 1);
  }

  async function send(files: FileList | null) {
    if (!files || files.length === 0) return;
    busy = true; error = '';
    try {
      for (const f of Array.from(files)) {
        const kind: Kind = f.type.startsWith('image/') ? 'photo' : 'document';
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
    } catch (e) { error = $t((e as Error).message); } finally { busy = false; el.value = ''; cam.value = ''; }
  }
</script>

<div class="picker">
  <input bind:this={el} type="file" multiple accept="image/*,application/pdf,.txt,.md,.doc,.docx,.xls,.xlsx"
         onchange={(e) => send((e.currentTarget as HTMLInputElement).files)} />
  <!-- `capture` cannot live on the input above: on mobile it suppresses picking an existing file. -->
  <input bind:this={cam} type="file" accept="image/*" capture="environment"
         onchange={(e) => send((e.currentTarget as HTMLInputElement).files)} />
  <div class="row">
    <button type="button" class="ghost" disabled={busy} onclick={() => el.click()}>
      {busy ? $t('activity.uploading') : `+ ${$t('activity.add-files')}`}
    </button>
    <button type="button" class="ghost" disabled={busy} onclick={() => cam.click()}>📷 {$t('activity.take-photo')}</button>
  </div>
  {#if error}<p class="error">{error}</p>{/if}
</div>

<style>
  .picker input { display: none; }
  .picker button { border: 1px dashed var(--border); width: 100%; }
</style>
