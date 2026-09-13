<script lang="ts">
  import TopBar from '../../lib/TopBar.svelte';
  import { uploadRaw } from '../../lib/api';
  import { t } from '../../i18n';
  import type { ImportCounts } from '../../lib/types';

  let fileEl: HTMLInputElement;
  let message = $state('');
  let error = $state('');

  async function doImport(files: FileList | null) {
    if (!files || files.length === 0) return;
    try {
      const counts = await uploadRaw<ImportCounts>('/import', files[0], 'application/zip');
      message = $t('settings.import-done', counts as unknown as Record<string, number>);
    } catch (e) { error = (e as Error).message; } finally { fileEl.value = ''; }
  }
</script>

<main>
  <TopBar title={$t('settings.data')} backTo="/settings" />
  {#if error}<p class="error">{error}</p>{/if}
  {#if message}<p class="muted">{message}</p>{/if}

  <div class="list">
    <a class="button-like" href="/api/export">{$t('settings.export')}</a>
    <button onclick={() => fileEl.click()}>{$t('settings.import')}</button>
    <input bind:this={fileEl} type="file" accept=".zip,application/zip" hidden onchange={(e) => doImport((e.currentTarget as HTMLInputElement).files)} />
  </div>
  <!-- Beside the button, not in the Backup section: the export is the thing somebody reaches
       for when they mean "keep a copy", and it is genuinely useful -- just not a backup of the
       database. -->
  <p class="muted">{$t('settings.export-not-backup')}</p>
</main>
