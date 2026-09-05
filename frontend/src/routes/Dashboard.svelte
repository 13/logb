<script lang="ts">
  import TopBar from '../lib/TopBar.svelte';
  import { t } from '../i18n';
</script>

<main>
  <TopBar title={$t('dash.title')} showSettings />
  <p class="muted">{$t('dash.empty')}</p>
</main>
