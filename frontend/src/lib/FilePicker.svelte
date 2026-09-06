<script lang="ts">
  import { upload } from './api';
  import { t } from '../i18n';
  import type { Attachment } from './types';

  let { objectId, activityId = null, onuploaded }: { objectId: number; activityId?: number | null; onuploaded: (a: Attachment) => void } = $props();
  let busy = $state(false);
  let error = $state('');
  let el: HTMLInputElement;
  let cam: HTMLInputElement;

  async function send(files: FileList | null) {
    if (!files || files.length === 0) return;
    busy = true; error = '';
    try {
      for (const f of Array.from(files)) {
        const form = new FormData();
        form.append('file', f, f.name);
        if (activityId !== null) form.append('activity_id', String(activityId));
        // Each file gets its own id, so a multi-file selection can't collide on the server's
        // idempotency key (see `client_op_id` handling in src/api/attachments.rs) -- without
        // this the whole attachment half of that idempotency work is unreachable from any
        // client, and a response lost after the server has already stored the file produces a
        // genuine duplicate attachment on the retry a user does by hand.
        //
        // This id does NOT survive a retry: a failed upload here just sets `error` and clears
        // the <input> (see `el.value = ''` below), so retrying means re-picking the file, which
        // hands back an unrelated File object with nothing reliable to recognise it as "the
        // same" upload as before (name/size/lastModified are the closest proxy, but two
        // legitimately different files can share all three). Reusing the id across a retry
        // would need this component restructured into a queue that keeps the original File
        // (and its id) around until the upload is confirmed, rather than firing and forgetting
        // per pick as it does today. So each retry attempt gets a fresh id per file instead.
        form.append('client_op_id', crypto.randomUUID());
        onuploaded(await upload<Attachment>(`/objects/${objectId}/attachments`, form));
      }
    } catch (e) { error = (e as Error).message; } finally { busy = false; el.value = ''; cam.value = ''; }
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
