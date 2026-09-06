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
