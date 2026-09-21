<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { api } from '../../lib/api';
  import { locale, navigatorLangs, t } from '../../i18n';
  import { LANG_NAMES, SUPPORTED } from '../../i18n/detect';
  import { settings } from '../../stores/settings';
  import { currency, rememberCurrentCurrency, user } from '../../stores/session';
  import { DATE_FORMATS, fmtDate, resolveDateFormat, type DateFormat } from '../../lib/format';
  import type { Settings } from '../../lib/types';

  const EXAMPLE = '2026-09-15';
  /** What `auto` itself would resolve to right now -- shown in its own label so picking
   *  "Automatic" is not a leap of faith about what it means on this device. `navigatorLangs`
   *  (not an imperative `navigatorTags()` call) so a `languagechange` -- a region change alone
   *  included -- updates this label live. */
  const resolvedAuto = $derived(resolveDateFormat('auto', $locale, $navigatorLangs));
  const dateFormatLabel = (id: (typeof DATE_FORMATS)[number]): string =>
    id === 'auto'
      ? $t('settings.date-format-auto', { example: fmtDate(EXAMPLE, resolvedAuto) })
      : fmtDate(EXAMPLE, id as DateFormat);

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
      // Otherwise an offline start right after this change would show the currency this device
      // knew before it, until the next successful /settings load remembers it again.
      rememberCurrentCurrency(s.currency);
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

  <h2>{$t('settings.date-format')}</h2>
  <div class="field">
    <select bind:value={$settings.dateFormat} aria-label={$t('settings.date-format')}>
      {#each DATE_FORMATS as id (id)}<option value={id}>{dateFormatLabel(id)}</option>{/each}
    </select>
  </div>

  <h2>{$t('settings.first-day')}</h2>
  <div class="field">
    <select bind:value={$settings.firstDayOfWeek} aria-label={$t('settings.first-day')}>
      <option value="locale">{$t('settings.first-day-auto')}</option>
      <option value="monday">{$t('weekday.1')}</option>
      <option value="sunday">{$t('weekday.7')}</option>
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
