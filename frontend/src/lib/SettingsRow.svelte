<script lang="ts">
  import Icon from './Icon.svelte';
  import { go } from './router';
  import { t } from '../i18n';
  import type { SettingsRowModel } from './settings-rows';

  let { row }: { row: SettingsRowModel } = $props();
</script>

<button class="settings-row" onclick={() => go(row.path)}>
  <span class="icon"><Icon name={row.icon} /></span>
  <span class="label">{$t(row.label)}</span>
  <!-- A row with no value renders no element at all, rather than an empty one: an empty span
       still takes its grid column and leaves the chevron sitting away from the edge, which
       reads as a value that failed to load rather than one that was never there. -->
  {#if row.value}<span class="value muted">{row.value}</span>{/if}
  <span class="chev muted"><Icon name="chevron" size={18} /></span>
</button>

<style>
  .settings-row {
    display: grid; grid-template-columns: auto 1fr auto auto;
    align-items: center; gap: var(--space-3);
    width: 100%; text-align: left; background: var(--surface);
    border: 1px solid var(--border); border-radius: var(--radius-md);
    padding: var(--space-3); min-height: var(--control);
  }
  .icon { display: flex; color: var(--muted); }
  .value { font-size: var(--text-sm); }
  .chev { display: flex; }
</style>
