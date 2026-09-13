<script lang="ts">
  import { t } from '../i18n';
</script>

<!-- The version, and nothing else. `__APP_VERSION__` is a build-time global (see vite.config.ts)
     and was already used for exactly this string at the bottom of Settings. -->
<p class="version muted">{$t('settings.version')} {__APP_VERSION__}</p>

<style>
  .version { font-size: var(--text-xs); }
</style>
