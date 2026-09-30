<script lang="ts">
  import { t } from '../i18n';
  import { applyUpdate, dismissUpdate, updateReady } from './sw-update';
</script>

<!-- A new build is waiting (see ./sw-update.ts). Offered, never forced: reloading on its own used
     to throw away a half-filled form. Mounted beside the app by main.ts, so it shows on every
     screen, the sign-in card included. -->
{#if $updateReady}
  <div role="status"
       class="fixed inset-x-4 top-[calc(0.5rem+env(safe-area-inset-top))] z-20 mx-auto flex max-w-[560px] flex-wrap items-center gap-2 rounded-lg border border-border bg-popover px-3 py-2 text-popover-foreground shadow-lg">
    <span class="min-w-0 flex-[1_1_12rem] text-sm">{$t('app.update-ready')}</span>
    <button data-slot="update-later" onclick={dismissUpdate}
            class="inline-flex min-h-11 shrink-0 cursor-pointer items-center rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">{$t('app.update-later')}</button>
    <button data-slot="update-reload" onclick={() => void applyUpdate()}
            class="inline-flex min-h-11 shrink-0 cursor-pointer items-center rounded-md bg-primary px-3 text-sm font-semibold text-primary-foreground hover:bg-primary/80 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">{$t('app.update-reload')}</button>
  </div>
{/if}
