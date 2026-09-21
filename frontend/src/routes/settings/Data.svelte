<script lang="ts">
  import TopBar from '../../lib/TopBar.svelte';
  import { discardDeadOp, deadOps, outboxPending, retryDead, uploadRaw } from '../../lib/api';
  import type { QueuedOp } from '../../lib/outbox';
  import { t } from '../../i18n';
  import type { ImportCounts } from '../../lib/types';

  let fileEl: HTMLInputElement;
  let message = $state('');
  let error = $state('');
  let excludeBody = $state(false);
  let pending = $state(0);
  let failed = $state<QueuedOp[]>([]);
  let recoveryBusy = $state(false);

  async function loadRecovery() {
    pending = await outboxPending();
    failed = await deadOps();
  }

  async function retryFailed() {
    recoveryBusy = true;
    try { await retryDead(); await loadRecovery(); }
    catch (e) { error = (e as Error).message; }
    finally { recoveryBusy = false; }
  }

  async function discard(id: string) {
    await discardDeadOp(id);
    await loadRecovery();
  }

  $effect(() => { void loadRecovery(); });

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
    <label><input type="checkbox" bind:checked={excludeBody} /> {$t('settings.export-exclude-body')}</label>
    <a class="button-like" href={excludeBody ? '/api/export?exclude_body=true' : '/api/export'}>{$t('settings.export')}</a>
    <button onclick={() => fileEl.click()}>{$t('settings.import')}</button>
    <input bind:this={fileEl} type="file" accept=".zip,application/zip" hidden onchange={(e) => doImport((e.currentTarget as HTMLInputElement).files)} />
  </div>
  <section class="recovery" aria-labelledby="recovery-title">
    <h2 id="recovery-title">{$t('settings.sync-recovery')}</h2>
    {#if pending > 0}<p class="muted">{$t('settings.sync-pending', { n: pending })}</p>{/if}
    {#if failed.length === 0}
      <p class="muted">{$t('settings.sync-clear')}</p>
    {:else}
      <p class="error">{$t('settings.sync-failed', { n: failed.length })}</p>
      <ul>
        {#each failed as op (op.id)}
          <li>
            <span>{op.kind}</span>
            <span class="muted">{op.lastError ?? $t('error.generic')}</span>
            <button class="ghost" disabled={recoveryBusy} onclick={() => discard(op.id)}>{$t('settings.sync-discard')}</button>
          </li>
        {/each}
      </ul>
      <button class="primary" disabled={recoveryBusy} onclick={retryFailed}>{$t('settings.sync-retry')}</button>
    {/if}
  </section>
  <!-- Beside the button, not in the Backup section: the export is the thing somebody reaches
       for when they mean "keep a copy", and it is genuinely useful -- just not a backup of the
       database. -->
  <p class="muted">{$t('settings.export-not-backup')}</p>
</main>

<style>
  .recovery { margin-top: var(--space-6); }
  .recovery ul { display: grid; gap: var(--space-2); padding-left: var(--space-4); }
  .recovery li { display: flex; gap: var(--space-2); align-items: center; flex-wrap: wrap; }
  .recovery li .muted { flex: 1 1 16rem; }
</style>
