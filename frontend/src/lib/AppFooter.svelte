<script lang="ts">
  import SignedIn from './SignedIn.svelte';
  import { t } from '../i18n';
</script>

<!-- The sidebar's foot: who is signed in, a one-click way out, and the version. Desktop only --
     a phone has no footer, and finds all three in Settings instead. `__APP_VERSION__` is a
     build-time global (see vite.config.ts). -->
<div class="foot">
  <SignedIn compact />
  <p class="version muted">{$t('settings.version')} {__APP_VERSION__}</p>
</div>

<style>
  .foot { display: flex; flex-direction: column; gap: var(--space-2); }
  .version { font-size: var(--text-xs); }
</style>
