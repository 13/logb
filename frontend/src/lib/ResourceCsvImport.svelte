<script lang="ts">
  import { createQueued } from './api';
  import { parseResourceCsv } from './resource-csv';
  import { t } from '../i18n';
  import type { MeasurementMode } from './types';
  let { objectId, mode = 'usage', onimported }: { objectId: number; mode?: MeasurementMode; onimported?: () => void } = $props();
  let file = $state<File | null>(null); let count = $state(0); let error = $state(''); let busy = $state(false);
  async function selected(e: Event) {
    file = (e.currentTarget as HTMLInputElement).files?.[0] ?? null; count = 0; error = '';
    if (!file) return;
    try { count = parseResourceCsv(await file.text(), mode).length; } catch (e) { error = (e as Error).message; }
  }
  async function run() {
    if (!file) return; busy = true; error = '';
    try {
      const entries = parseResourceCsv(await file.text(), mode);
      for (const body of entries) await createQueued(`/objects/${objectId}/activities`, body as unknown as Record<string, unknown>);
      file = null; count = 0; onimported?.();
    } catch (e) { error = (e as Error).message; } finally { busy = false; }
  }
</script>
<section>
  <h3>{$t('resource.csv-title')}</h3>
  <p class="hint">{$t('resource.csv-hint')}</p>
  <input aria-label={$t('resource.csv-file')} type="file" accept=".csv,text/csv" onchange={selected} />
  {#if count > 0}<button class="ghost" disabled={busy} onclick={run}>{$t('resource.csv-import', { n: count })}</button>{/if}
  {#if error}<p class="error">{error}</p>{/if}
</section>
