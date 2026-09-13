<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import { LANG_NAMES, SUPPORTED } from '../../i18n/detect';
  import { settings } from '../../stores/settings';
  import { currency, user } from '../../stores/session';

  let currencyText = $state('');
  let message = $state('');
  let error = $state('');

  const isAdmin = $derived($user?.is_admin === true);

  onMount(() => {
    currencyText = $currency;
  });

  async function saveCurrency() {
    try {
      const s = await api<{ currency: string }>('PUT', '/settings', { currency: currencyText.trim().toUpperCase() });
      currency.set(s.currency);
      message = $t('object.saved');
    } catch (e) { error = (e as Error).message; }
  }
</script>

<main>
  <TopBar title={$t('settings.appearance')} backTo="/settings" />
  {#if error}<p class="error">{error}</p>{/if}
  {#if message}<p class="muted">{message}</p>{/if}

  <h2>{$t('settings.language')}</h2>
  <div class="field">
    <select bind:value={$settings.locale} aria-label={$t('settings.language')}>
      <option value="auto">{$t('settings.language-auto')}</option>
      {#each SUPPORTED as l}<option value={l}>{LANG_NAMES[l]}</option>{/each}
    </select>
  </div>

  <h2>{$t('settings.theme')}</h2>
  <div class="field">
    <select bind:value={$settings.theme} aria-label={$t('settings.theme')}>
      <option value="auto">{$t('settings.theme-auto')}</option>
      <option value="light">{$t('settings.theme-light')}</option>
      <option value="dark">{$t('settings.theme-dark')}</option>
    </select>
  </div>

  {#if isAdmin}
    <h2>{$t('settings.currency')}</h2>
    <div class="row">
      <div class="field"><input bind:value={currencyText} maxlength="3" aria-label={$t('settings.currency')} /><span class="hint">{$t('settings.currency-hint')}</span></div>
      <button onclick={saveCurrency}>{$t('nav.save')}</button>
    </div>
  {/if}
</main>

<style>
  .row > button { flex: none; }
</style>
