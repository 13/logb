<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { get } from 'svelte/store';
  import TopBar from '../../lib/TopBar.svelte';
  import Toaster from '../../lib/Toaster.svelte';
  import { toast } from '../../lib/toast';
  import { autosave } from '../../lib/autosave';
  import { errorMessage } from '../../lib/api-error';
  import { api } from '../../lib/api';
  import { Button } from '$lib/components/ui/button/index.js';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { errorClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { locale, navigatorLangs, t } from '../../i18n';
  import { LANG_NAMES, SUPPORTED } from '../../i18n/detect';
  import { settings, appearance, setDeviceOverride, applyAccountAppearance, type LocalSettings } from '../../stores/settings';
  import { currency, rememberCurrentCurrency, user } from '../../stores/session';
  import { DATE_FORMATS, fmtDate, resolveDateFormat, type DateFormat } from '../../lib/format';
  import type { Settings } from '../../lib/types';

  const EXAMPLE = '2026-09-15';
  /** What `auto` resolves to right now, shown in its own label. `navigatorLangs` so a
   *  `languagechange` updates it live. */
  const resolvedAuto = $derived(resolveDateFormat('auto', $locale, $navigatorLangs));
  const dateFormatLabel = (id: (typeof DATE_FORMATS)[number]): string =>
    id === 'auto'
      ? $t('settings.date-format-auto', { example: fmtDate(EXAMPLE, resolvedAuto) })
      : fmtDate(EXAMPLE, id as DateFormat);

  let error = $state('');
  // Read from the store itself, so the box follows the stored record.
  const overridden = $derived(!!$appearance[String($user?.id)]?.override);

  /** The account's appearance, a moment after the last change, one request at a time. The answer
   *  is applied only if nothing changed while it was out (`issuedWith`). */
  const accountSave = autosave<LocalSettings>(async (value, opts) => {
    const me = get(user);
    if (!me) return;
    const saved = await api<LocalSettings>('PUT', '/me/appearance', value, undefined, opts);
    applyAccountAppearance(me.id, saved, value);
  }, {
    onsaved: () => { error = ''; toast($t('object.saved')); },
    onerror: (e) => { error = errorMessage(e, $t); },
  });

  /** A setting applies at once (the store is what the app reads) and is saved to the account,
   *  unless this device keeps its own. */
  function set<K extends keyof LocalSettings>(key: K, value: LocalSettings[K]) {
    settings.update((s) => ({ ...s, [key]: value }));
    if (overridden) toast($t('settings.saved-device'));
    else accountSave.push(get(settings));
  }

  /** On: changes stay here. Off: this device follows the account again, and what it shows now
   *  becomes the account's -- what "Apply appearance" used to do. */
  function setOverride(enabled: boolean) {
    const me = get(user);
    if (!me) return;
    setDeviceOverride(me.id, enabled);
    if (enabled) toast($t('settings.saved-device'));
    else accountSave.push(get(settings));
  }

  const isAdmin = $derived($user?.is_admin === true);
  /** Every zone the browser knows, when it can say; a plain text field otherwise. */
  const zones: string[] = (Intl as unknown as { supportedValuesOf?: (k: string) => string[] }).supportedValuesOf?.('timeZone') ?? [];

  let currencyText = $state('');
  let currencyError = $state('');
  let instanceError = $state('');
  let timezone = $state('');
  let timezoneLocked = $state(false);
  let instanceLoaded = $state(false);

  const instanceSave = autosave<Record<string, string>>(async (body, opts) => {
    const s = await api<Settings>('PUT', '/settings', body, undefined, opts);
    currency.set(s.currency);
    // Otherwise an offline start right after this change would show the old currency.
    rememberCurrentCurrency(s.currency);
  }, {
    onsaved: () => { instanceError = ''; toast($t('object.saved')); },
    onerror: (e) => { instanceError = errorMessage(e, $t); },
  });

  /** Currency and timezone go together, as the server takes them; a currency that cannot be one
   *  is refused here, under its field, and nothing is sent. */
  function saveInstance() {
    const code = currencyText.trim().toUpperCase();
    if (!/^[A-Z]{3}$/.test(code)) { currencyError = $t('settings.currency-invalid'); return; }
    currencyError = '';
    currencyText = code;
    const body: Record<string, string> = { currency: code };
    if (!timezoneLocked && timezone.trim()) body.timezone = timezone.trim();
    instanceSave.push(body);
  }

  onMount(async () => {
    currencyText = $currency;
    if (!isAdmin) return;
    try {
      const s = await api<Settings>('GET', '/settings');
      timezone = s.timezone;
      timezoneLocked = s.timezone_locked;
    } catch { /* the field stays empty and saving leaves the timezone alone */ }
    instanceLoaded = true;
  });
  // A change made just before leaving still reaches the server: on a route change (destroy) and
  // when the tab or app is closed (pagehide).
  const flushAll = () => { void accountSave.flush(); void instanceSave.flush(); };
  onDestroy(flushAll);

  const card = 'flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs';
</script>

<svelte:window onpagehide={flushAll} />

<main>
  <TopBar title={$t('settings.appearance')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    {#if error}
      <div class="flex flex-wrap items-center gap-3">
        <p role="alert" class={errorClass}>{error}</p>
        <Button variant="outline" class="min-h-11" onclick={accountSave.retry}>{$t('outbox.retry')}</Button>
      </div>
    {/if}

    <section aria-labelledby="appearance-you" class={card}>
      <h2 id="appearance-you" class={sectionHeadingClass}>{$t('settings.you')}</h2>
      <Field id="set-language" label={$t('settings.language')}>
        <NativeSelect bind:value={() => $settings.locale, (v) => set('locale', v as LocalSettings['locale'])}>
          <option value="auto">{$t('settings.language-auto')}</option>
          {#each SUPPORTED as l (l)}<option value={l}>{LANG_NAMES[l]}</option>{/each}
        </NativeSelect>
      </Field>
      <Field id="set-date-format" label={$t('settings.date-format')}>
        <NativeSelect bind:value={() => $settings.dateFormat, (v) => set('dateFormat', v as LocalSettings['dateFormat'])}>
          {#each DATE_FORMATS as id (id)}<option value={id}>{dateFormatLabel(id)}</option>{/each}
        </NativeSelect>
      </Field>
      <Field id="set-first-day" label={$t('settings.first-day')}>
        <NativeSelect bind:value={() => $settings.firstDayOfWeek, (v) => set('firstDayOfWeek', v as LocalSettings['firstDayOfWeek'])}>
          <option value="locale">{$t('settings.first-day-auto')}</option>
          <option value="monday">{$t('weekday.1')}</option>
          <option value="sunday">{$t('weekday.7')}</option>
        </NativeSelect>
      </Field>
      <Field id="set-theme" label={$t('settings.theme')}>
        <NativeSelect bind:value={() => $settings.theme, (v) => set('theme', v as LocalSettings['theme'])}>
          <option value="auto">{$t('settings.theme-auto')}</option>
          <option value="light">{$t('settings.theme-light')}</option>
          <option value="dark">{$t('settings.theme-dark')}</option>
        </NativeSelect>
      </Field>
      <CheckField id="set-device-only" label={$t('settings.device-only')} hint={$t('settings.device-only-hint')}
                  bind:checked={() => overridden, (on) => setOverride(on)} />
    </section>

    {#if isAdmin}
      <section aria-labelledby="appearance-instance" aria-busy={!instanceLoaded} class={card}>
        <h2 id="appearance-instance" class={sectionHeadingClass}>{$t('settings.instance')}</h2>
        <Field id="set-currency" label={$t('settings.currency')} hint={$t('settings.currency-hint')} error={currencyError}>
          <!-- `bind:value` follows typing (input); saving waits for `change` (blur or Enter). -->
          <Input bind:value={currencyText} maxlength={3} autocomplete="off" autocapitalize="characters" spellcheck="false" onchange={saveInstance} />
        </Field>
        <Field id="set-timezone" label={$t('settings.timezone')} hint={timezoneLocked ? $t('settings.timezone-locked') : $t('settings.timezone-hint')}>
          {#if zones.length > 0}
            <NativeSelect disabled={timezoneLocked} bind:value={() => timezone, (v) => { timezone = v ?? ''; saveInstance(); }}>
              <!-- The current value stays selectable even when this browser's list lacks it. -->
              {#if timezone && !zones.includes(timezone)}<option value={timezone}>{timezone}</option>{/if}
              {#each zones as z (z)}<option value={z}>{z}</option>{/each}
            </NativeSelect>
          {:else}
            <Input bind:value={timezone} disabled={timezoneLocked} autocomplete="off" onchange={saveInstance} />
          {/if}
        </Field>
        {#if instanceError}
          <div class="flex flex-wrap items-center gap-3">
            <p role="alert" class={errorClass}>{instanceError}</p>
            <Button variant="outline" class="min-h-11" onclick={instanceSave.retry}>{$t('outbox.retry')}</Button>
          </div>
        {/if}
      </section>
    {/if}
  </div>
  <Toaster />
</main>
