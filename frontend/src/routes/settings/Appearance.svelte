<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import { LANG_NAMES, SUPPORTED } from '../../i18n/detect';
  import { settings } from '../../stores/settings';
  import { currency, user } from '../../stores/session';
  import type { Settings } from '../../lib/types';

  let currencyText = $state('');
  let timezone = $state('');
  let timezoneLocked = $state(false);
  let message = $state('');
  let error = $state('');

  const isAdmin = $derived($user?.is_admin === true);
  /** Every zone the browser knows, when it can say; a plain text field otherwise. */
  const zones: string[] = (Intl as unknown as { supportedValuesOf?: (k: string) => string[] }).supportedValuesOf?.('timeZone') ?? [];

  onMount(async () => {
    currencyText = $currency;
    if (!isAdmin) return;
    try {
      const s = await api<Settings>('GET', '/settings');
      timezone = s.timezone;
      timezoneLocked = s.timezone_locked;
    } catch { /* the field stays empty and saving leaves the timezone alone */ }
  });

  async function saveInstance() {
    error = ''; message = '';
    try {
      const body: Record<string, string> = { currency: currencyText.trim().toUpperCase() };
      if (!timezoneLocked && timezone.trim()) body.timezone = timezone.trim();
      const s = await api<Settings>('PUT', '/settings', body);
      currency.set(s.currency);
      timezone = s.timezone;
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
    <div class="field"><input bind:value={currencyText} maxlength="3" aria-label={$t('settings.currency')} /><span class="hint">{$t('settings.currency-hint')}</span></div>

    <h2>{$t('settings.timezone')}</h2>
    <div class="field">
      {#if zones.length > 0}
        <select bind:value={timezone} aria-label={$t('settings.timezone')} disabled={timezoneLocked}>
          <!-- The current value stays selectable even when this browser's list lacks it. -->
          {#if timezone && !zones.includes(timezone)}<option value={timezone}>{timezone}</option>{/if}
          {#each zones as z}<option value={z}>{z}</option>{/each}
        </select>
      {:else}
        <input bind:value={timezone} aria-label={$t('settings.timezone')} disabled={timezoneLocked} />
      {/if}
      <span class="hint">{timezoneLocked ? $t('settings.timezone-locked') : $t('settings.timezone-hint')}</span>
    </div>
    <button onclick={saveInstance}>{$t('nav.save')}</button>
  {/if}
</main>
