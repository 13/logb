<script lang="ts">
  import { t } from '../i18n';
  import { applyUpdate, dismissUpdate, updateReady } from './sw-update';
</script>

<!-- A new build is waiting (see ./sw-update.ts). Offered, never forced: reloading on its own
     used to throw away a half-filled form. Mounted beside the app by main.ts, so it shows on
     every screen, the login page included. -->
{#if $updateReady}
  <div class="update" role="status">
    <span>{$t('app.update-ready')}</span>
    <button class="ghost" onclick={dismissUpdate}>{$t('app.update-later')}</button>
    <button class="primary" onclick={() => void applyUpdate()}>{$t('app.update-reload')}</button>
  </div>
{/if}

<style>
  .update {
    position: fixed; left: var(--space-4); right: var(--space-4);
    top: calc(var(--space-2) + env(safe-area-inset-top));
    z-index: 20; max-width: 560px; margin: 0 auto;
    display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap;
    padding: var(--space-2) var(--space-3);
    background: var(--surface); color: var(--text);
    border: 1px solid var(--border); border-radius: var(--radius-md);
    box-shadow: 0 4px 12px rgba(0,0,0,.25);
  }
  .update span { flex: 1 1 12rem; font-size: var(--text-sm); }
  .update button { flex: none; }
</style>
