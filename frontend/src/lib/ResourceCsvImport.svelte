<script lang="ts">
  import { errorMessage } from './api-error';
  import { createQueued } from './api';
  import { parseResourceCsv } from './resource-csv';
  import { t } from '../i18n';
  import { Button } from '$lib/components/ui/button/index.js';
  import type { MeasurementMode } from './types';
  let { objectId, mode = 'usage', onimported }: { objectId: number; mode?: MeasurementMode; onimported?: () => void } = $props();
  let file = $state<File | null>(null); let count = $state(0); let error = $state(''); let busy = $state(false);
  async function selected(e: Event) {
    file = (e.currentTarget as HTMLInputElement).files?.[0] ?? null; count = 0; error = '';
    if (!file) return;
    try { count = parseResourceCsv(await file.text(), mode).length; } catch (e) { error = errorMessage(e, $t); }
  }
  async function run() {
    if (!file) return; busy = true; error = '';
    try {
      const entries = parseResourceCsv(await file.text(), mode);
      for (const body of entries) await createQueued(`/objects/${objectId}/activities`, body as unknown as Record<string, unknown>);
      file = null; count = 0; onimported?.();
    } catch (e) { error = errorMessage(e, $t); } finally { busy = false; }
  }
</script>
<section class="flex flex-col gap-2">
  <h3 class="m-0 mt-4 text-sm font-semibold text-foreground">{$t('resource.csv-title')}</h3>
  <p class="m-0 text-sm text-muted-foreground">{$t('resource.csv-hint')}</p>
  <!-- The browser's own file control, its button drawn like an outline button. -->
  <input data-slot="csv-file" aria-label={$t('resource.csv-file')} type="file" accept=".csv,text/csv" onchange={selected}
         class="block w-full text-sm text-muted-foreground file:mr-3 file:min-h-11 file:cursor-pointer file:rounded-lg file:border file:border-input file:bg-card file:px-3 file:text-sm file:font-medium file:text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring" />
  {#if count > 0}<Button variant="outline" class="min-h-11 w-fit" disabled={busy} onclick={run}>{$t('resource.csv-import', { n: count })}</Button>{/if}
  {#if error}<p class="m-0 text-sm font-medium text-destructive" role="alert">{error}</p>{/if}
</section>
