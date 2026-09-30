<script lang="ts">
  import { errorMessage } from '../../lib/api-error';
  import { onDestroy } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { discardDeadOp, deadOps, outboxPending, retryDead, uploadRaw } from '../../lib/api';
  import type { QueuedOp } from '../../lib/outbox';
  import { describeFailedWrite } from '../../lib/failed-write';
  import { t } from '../../i18n';
  import Toaster from '../../lib/Toaster.svelte';
  import { toast } from '../../lib/toast';
  import { Button } from '$lib/components/ui/button/index.js';
  import { CheckField } from '$lib/components/ui/field/index.js';
  import { errorClass, hintClass, sectionHeadingClass, destructiveGhostClass } from '$lib/components/ui/field/classes.js';
  import type { ImportCounts } from '../../lib/types';

  let fileEl: HTMLInputElement;
  /** Set when the page is left: an answer that comes back later must not toast on the next page. */
  let left = false;
  onDestroy(() => (left = true));
  let error = $state('');
  let excludeBody = $state(false);
  let pending = $state(0);
  let failed = $state<QueuedOp[]>([]);
  let recoveryBusy = $state(false);

  const card = 'flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs';

  async function loadRecovery() {
    pending = await outboxPending();
    failed = await deadOps();
  }

  async function retryFailed() {
    recoveryBusy = true;
    try { await retryDead(); await loadRecovery(); }
    catch (e) { error = errorMessage(e, $t); }
    finally { recoveryBusy = false; }
  }

  /** As on the hub: a discarded save is gone for good, so it asks first. */
  async function discard(id: string) {
    if (!confirm($t('nav.confirm-delete'))) return;
    await discardDeadOp(id);
    await loadRecovery();
  }

  $effect(() => { void loadRecovery(); });

  async function doImport(files: FileList | null) {
    if (!files || files.length === 0) return;
    try {
      const counts = await uploadRaw<ImportCounts>('/import', files[0], 'application/zip');
      if (left) return;
      toast($t('settings.import-done', counts as unknown as Record<string, number>), 8000);
    } catch (e) { if (!left) error = errorMessage(e, $t); } finally { fileEl.value = ''; }
  }
</script>

<main>
  <TopBar title={$t('settings.data')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}

    <section aria-labelledby="export-title" class={card}>
      <h2 id="export-title" class={sectionHeadingClass}>{$t('settings.export-title')}</h2>
      <CheckField id="export-exclude-body" label={$t('settings.export-exclude-body')} bind:checked={excludeBody} />
      <div class="flex flex-wrap gap-2">
        <!-- The page's one primary action: a link, so the browser downloads it. -->
        <Button href={excludeBody ? '/api/export?exclude_body=true' : '/api/export'} class="h-12">{$t('settings.export')}</Button>
        <Button variant="outline" class="h-12" onclick={() => fileEl.click()}>{$t('settings.import')}</Button>
      </div>
      <input bind:this={fileEl} data-slot="import-file" type="file" accept=".zip,application/zip" hidden
             onchange={(e) => doImport((e.currentTarget as HTMLInputElement).files)} />
      <!-- Beside the buttons, not in the Backup section: the export is what somebody reaches for
           when they mean "keep a copy", and it is not a database backup. -->
      <p class={hintClass}>{$t('settings.export-not-backup')}</p>
    </section>

    <section aria-labelledby="recovery-title" class={card}>
      <h2 id="recovery-title" class={sectionHeadingClass}>{$t('settings.sync-recovery')}</h2>
      {#if pending > 0}<p class={hintClass}>{$t('settings.sync-pending', { n: pending })}</p>{/if}
      {#if failed.length === 0}
        <p class={hintClass}>{$t('settings.sync-clear')}</p>
      {:else}
        <p role="alert" class={errorClass}>{$t('settings.sync-failed', { n: failed.length })}</p>
        <ul role="list" class="m-0 flex list-none flex-col gap-2 p-0">
          {#each failed as op (op.id)}
            <!-- Described as on the hub: what it was, its name, and why the server refused it. -->
            {@const d = describeFailedWrite(op, $t)}
            <li data-testid="failed-write" class="flex items-center gap-3 rounded-md border border-border p-3">
              <span class="flex min-w-0 flex-1 flex-col gap-1 [overflow-wrap:anywhere]">
                <b class="font-semibold text-foreground">{d.what}{#if d.name}: {d.name}{/if}</b>
                {#if d.reason}<span class="text-sm text-destructive">{d.reason}</span>{/if}
              </span>
              <Button variant="ghost" class={`min-h-11 shrink-0 ${destructiveGhostClass}`} disabled={recoveryBusy} onclick={() => discard(op.id)}>{$t('settings.sync-discard')}</Button>
            </li>
          {/each}
        </ul>
        <Button variant="outline" class="h-12 self-start" disabled={recoveryBusy} onclick={retryFailed}>{$t('settings.sync-retry')}</Button>
      {/if}
    </section>
  </div>
  <Toaster />
</main>
