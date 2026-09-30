<script lang="ts">
  import { errorMessage } from '../../lib/api-error';
  import { onDestroy, onMount } from 'svelte';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import TopBar from '../../lib/TopBar.svelte';
  import Toaster from '../../lib/Toaster.svelte';
  import { toast } from '../../lib/toast';
  import { autosave } from '../../lib/autosave';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { errorClass, hintClass, sectionHeadingClass, warnClass } from '$lib/components/ui/field/classes.js';
  import { disablePush, enablePush, pushState, type PushState } from '../../lib/push';
  import type { NotificationSettings, NotificationTest, TelegramLink } from '../../lib/types';

  let data = $state<NotificationSettings | null>(null);
  let push = $state<PushState | null>(null);
  let url = $state('');
  let format = $state<'text' | 'json'>('text');
  let hour = $state<number | null>(8);
  // '' means "the instance's timezone", which is what the server stores as null.
  let timezone = $state('');
  const zones: string[] = (Intl as unknown as { supportedValuesOf?: (k: string) => string[] }).supportedValuesOf?.('timeZone') ?? [];
  let busy = $state(false);
  let error = $state('');
  let telegramLink = $state<TelegramLink | null>(null);
  let telegramToken = $state('');
  let now = $state(Date.now());
  const telegramSeconds = $derived(telegramLink ? Math.max(0, Math.ceil((Date.parse(telegramLink.expires_at) - now) / 1000)) : 0);

  async function load() {
    data = await api<NotificationSettings>('GET', '/me/notifications');
  }

  onMount(() => {
    void (async () => {
      try {
        await load();
        url = data?.url ?? '';
        format = data?.format ?? 'text';
        hour = data?.hour ?? 8;
        timezone = data?.timezone ?? '';
      } catch (e) { error = errorMessage(e, $t); }
      push = await pushState();
    })();
  });

  // The countdown and the "has the bot been linked yet" poll, only while a link is pending: the
  // page otherwise ticked -- and re-rendered -- every two seconds for as long as it was open.
  $effect(() => {
    if (!telegramLink) return;
    const timer = setInterval(async () => {
      now = Date.now();
      if (!telegramLink) return;
      if (telegramSeconds === 0) { telegramLink = null; return; }
      try {
        await load();
        if (data?.telegram_connected) telegramLink = null;
      } catch { /* the page already reports explicit actions; a background refresh retries */ }
    }, 2000);
    return () => clearInterval(timer);
  });

  async function togglePush() {
    if (!data) return;
    busy = true; error = '';
    try {
      push = push === 'on' ? await disablePush() : await enablePush(data.vapid_public_key);
      await load();
    } catch (e) { error = errorMessage(e, $t); } finally { busy = false; }
  }

  async function sendTest() {
    busy = true; error = '';
    try {
      const r = await api<NotificationTest>('POST', '/me/notifications/test');
      const parts: string[] = [];
      if (r.push_sent + r.push_failed > 0) parts.push($t('notify.test-push', { n: r.push_sent }));
      // The server answers only `sent` or `failed` for each (the reason stays in its log).
      const outcome = (s: 'sent' | 'failed') => $t(s === 'sent' ? 'notify.test-sent' : 'notify.test-failed');
      if (r.webhook !== null) parts.push($t('notify.test-webhook', { result: outcome(r.webhook) }));
      if (r.telegram !== null) parts.push($t('notify.test-telegram', { result: outcome(r.telegram) }));
      toast(parts.length > 0 ? parts.join(' ') : $t('notify.test-nowhere'), 8000);
    } catch (e) { error = errorMessage(e, $t); } finally { busy = false; }
  }

  async function linkTelegram() {
    busy = true; error = '';
    try { telegramLink = await api<TelegramLink>('POST', '/me/notifications/telegram/link'); now = Date.now(); }
    catch (e) { error = errorMessage(e, $t); } finally { busy = false; }
  }

  async function saveTelegram() {
    busy = true; error = '';
    try {
      data = await api<NotificationSettings>('PUT', '/me/notifications/telegram', { token: telegramToken.trim() });
      telegramToken = ''; telegramLink = null; toast($t('object.saved'));
    } catch (e) { error = errorMessage(e, $t); } finally { busy = false; }
  }

  async function removeTelegram() {
    busy = true; error = '';
    try { await api('DELETE', '/me/notifications/telegram'); telegramToken = ''; telegramLink = null; await load(); }
    catch (e) { error = errorMessage(e, $t); } finally { busy = false; }
  }

  async function unlinkTelegram() {
    busy = true; error = '';
    try { await api('POST', '/me/notifications/telegram/unlink'); telegramLink = null; await load(); }
    catch (e) { error = errorMessage(e, $t); } finally { busy = false; }
  }

  let hourError = $state('');
  let urlError = $state('');
  /** Set when the page is left: an answer that comes back later must not write to a page that is gone. */
  let left = false;
  let hourFailed = $state(false);
  let urlFailed = $state(false);

  const hourSave = autosave<{ hour: number; timezone: string | null }>(async (body, opts) => {
    data = await api<NotificationSettings>('PUT', '/me/notifications/hour', body, undefined, opts);
  }, {
    onsaved: () => { if (left) return; hourError = ''; hourFailed = false; toast($t('object.saved')); },
    onerror: (e) => { if (left) return; hourError = errorMessage(e, $t); hourFailed = true; },
  });

  const webhookSave = autosave<{ url: string | null; format: 'text' | 'json' }>(async (body, opts) => {
    data = await api<NotificationSettings>('PUT', '/me/notifications', body, undefined, opts);
  }, {
    onsaved: () => { if (left) return; urlError = ''; urlFailed = false; toast($t('object.saved')); },
    onerror: (e) => { if (left) return; urlError = errorMessage(e, $t); urlFailed = true; },
  });

  // A change made just before leaving still reaches the server: on a route change (destroy) and
  // when the tab or app is closed (pagehide).
  const flushAll = () => { void hourSave.flush(); void webhookSave.flush(); };
  onDestroy(() => { left = true; flushAll(); });

  /** The hour and its timezone are one setting on the server. An hour that cannot be one is
   *  refused here, under its field. */
  function saveHour() {
    if (hour === null || !Number.isInteger(hour) || hour < 0 || hour > 23) { hourError = $t('notify.hour-invalid'); hourFailed = false; return; }
    hourError = '';
    hourSave.push({ hour, timezone: timezone || null });
  }

  /** A bad URL comes back from the server ("http or https") and stays under the field. */
  function saveWebhook() {
    webhookSave.push({ url: url.trim() || null, format });
  }

  const card = 'flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs';
</script>

<svelte:window onpagehide={flushAll} />

<main>
  <TopBar title={$t('settings.notifications')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6" aria-busy={data === null}>
    {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}

    <section aria-labelledby="notify-digest" class={card}>
      <h2 id="notify-digest" class={sectionHeadingClass}>{$t('notify.digest-title')}</h2>
      <Field id="delivery-hour" label={$t('notify.delivery-hour')} error={hourError}>
        <Input type="number" min={0} max={23} inputmode="numeric" bind:value={hour} onchange={saveHour} />
      </Field>
      <Field id="delivery-timezone" label={$t('notify.delivery-timezone')} hint={$t('notify.delivery-timezone-hint')}>
        {#if zones.length > 0}
          <NativeSelect bind:value={() => timezone, (v) => { timezone = v ?? ''; saveHour(); }}>
            <option value="">{$t('notify.instance-timezone')}</option>
            <!-- The stored value stays selectable even when this browser's list lacks it. -->
            {#if timezone && !zones.includes(timezone)}<option value={timezone}>{timezone}</option>{/if}
            {#each zones as z (z)}<option value={z}>{z}</option>{/each}
          </NativeSelect>
        {:else}
          <Input bind:value={timezone} placeholder={$t('notify.instance-timezone')} onchange={saveHour} />
        {/if}
      </Field>
      {#if hourFailed}<Button variant="outline" class="h-12 self-start" onclick={hourSave.retry}>{$t('outbox.retry')}</Button>{/if}
      {#if data?.deliveries?.length}
        <div class="flex flex-col gap-1">
          <h3 class={sectionHeadingClass}>{$t('notify.delivery-status')}</h3>
          <ul role="list" class="m-0 flex list-none flex-col gap-1 p-0 text-sm text-foreground">
            {#each data.deliveries as delivery}
              <li>
                {delivery.target.startsWith('telegram:') ? 'Telegram' : delivery.target.startsWith('push:') ? $t('notify.push-title') : $t('notify.webhook-title')}{#if delivery.last_success} · {$t('notify.delivery-ok')}: {delivery.last_success}{/if}{#if delivery.last_error}<span class="text-destructive"> · {$t('notify.delivery-error')}</span>{/if}
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    </section>

    <section aria-labelledby="notify-push" class={card}>
      <h2 id="notify-push" class={sectionHeadingClass}>{$t('notify.push-title')}</h2>
      {#if data}<p class={hintClass}>{$t('notify.push-hint', { hour: data.hour })}</p>{/if}
      {#if push === 'unsupported'}
        <p class={hintClass}>{$t('notify.push-unsupported')}</p>
      {:else if push === 'denied'}
        <p class={warnClass}>{$t('notify.push-denied')}</p>
      {:else if push}
        {#if push === 'on'}<p class="m-0 text-sm text-foreground">{$t('notify.push-on')}</p>{/if}
        <!-- The page's one primary action, while there is something to turn on. -->
        <Button variant={push === 'on' ? 'outline' : 'default'} class="h-12 self-start" onclick={togglePush} disabled={busy || !data}>
          {push === 'on' ? $t('notify.push-disable') : $t('notify.push-enable')}
        </Button>
      {/if}
      {#if data && data.push_devices > 0}
        <p class={hintClass}>{data.push_devices === 1 ? $t('notify.push-devices-one') : $t('notify.push-devices', { n: data.push_devices })}</p>
      {/if}
    </section>

    <section aria-labelledby="notify-telegram" class={card}>
      <h2 id="notify-telegram" class={sectionHeadingClass}>{$t('notify.telegram-title')}</h2>
      {#if !data?.telegram_configured}
        <p class={hintClass}>{$t('notify.telegram-setup')}</p>
        <Field id="telegram-token" label={$t('notify.telegram-token')}>
          <Input type="password" autocomplete="off" bind:value={telegramToken} />
        </Field>
        <Button variant="outline" class="h-12 self-start" onclick={saveTelegram} disabled={busy || !telegramToken.trim()}>{$t('notify.telegram-save')}</Button>
      {:else}
        <p class={hintClass}>{$t('notify.telegram-bot', { name: data.telegram_bot_username ? `@${data.telegram_bot_username}` : 'Telegram' })}</p>
        {#if data.telegram_legacy}<p class={warnClass}>{$t('notify.telegram-legacy')}</p>{/if}
        {#if data.telegram_connected}
          <p class="m-0 text-sm text-foreground">{$t('notify.telegram-connected', { name: data.telegram_display_name ?? 'Telegram' })}</p>
          {#if data.telegram_last_error}<p class={errorClass}>{data.telegram_last_error}</p>{/if}
          <Button variant="outline" class="h-12 self-start" onclick={unlinkTelegram} disabled={busy}>{$t('notify.telegram-disconnect')}</Button>
        {:else}
          <p class={hintClass}>{$t('notify.telegram-hint')}</p>
          {#if data.telegram_last_error}<p class={errorClass}>{data.telegram_last_error}</p>{/if}
          <Button variant="outline" class="h-12 self-start" onclick={linkTelegram} disabled={busy}>{$t('notify.telegram-connect')}</Button>
          {#if telegramLink}
            <div class="flex flex-col items-start gap-2">
              <!-- Server-generated SVG (src/domain/pairing.rs), never user input. -->
              <div class="w-60 max-w-full [&_svg]:block [&_svg]:h-auto [&_svg]:w-full" role="img" aria-label={$t('notify.telegram-title')}>{@html telegramLink.qr_svg}</div>
              <a data-slot="telegram-open" href={telegramLink.url} target="_blank" rel="noreferrer"
                 class="inline-flex min-h-11 items-center text-sm font-medium text-brand-ink underline underline-offset-4 focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">{$t('notify.telegram-open')}</a>
              <span class={hintClass}>{$t('notify.telegram-expires', { n: telegramSeconds })}</span>
            </div>
          {/if}
        {/if}
        {#if !data.telegram_legacy}
          <details class="group rounded-lg border border-border">
            <summary class="flex min-h-11 cursor-pointer list-none items-center gap-2 rounded-lg px-3 text-sm font-medium text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring [&::-webkit-details-marker]:hidden">
              <ChevronRight aria-hidden="true" class="size-4 transition-transform group-open:rotate-90" />{$t('notify.telegram-replace')}
            </summary>
            <div class="flex flex-col gap-3 px-3 pb-3">
              <Field id="telegram-replacement" label={$t('notify.telegram-token')}>
                <Input type="password" autocomplete="off" bind:value={telegramToken} />
              </Field>
              <Button variant="outline" class="h-12 self-start" onclick={saveTelegram} disabled={busy || !telegramToken.trim()}>{$t('notify.telegram-save')}</Button>
            </div>
          </details>
        {/if}
        <Button variant="destructive" class="h-12 self-start" onclick={removeTelegram} disabled={busy}>{$t('notify.telegram-remove')}</Button>
      {/if}
    </section>

    <section aria-labelledby="notify-webhook" class={card}>
      <h2 id="notify-webhook" class={sectionHeadingClass}>{$t('notify.webhook-title')}</h2>
      <p class={hintClass}>{data?.instance_webhook ? $t('notify.webhook-hint-instance') : $t('notify.webhook-hint')}</p>
      <Field id="wu" label={$t('notify.webhook-url')} error={urlError}>
        <Input type="url" inputmode="url" autocomplete="off" placeholder="https://ntfy.sh/…" bind:value={url} onchange={saveWebhook} />
      </Field>
      {#if urlFailed}<Button variant="outline" class="h-12 self-start" onclick={webhookSave.retry}>{$t('outbox.retry')}</Button>{/if}
      <Field id="wf" label={$t('notify.format')}>
        <NativeSelect bind:value={() => format, (v) => { format = v as 'text' | 'json'; saveWebhook(); }}>
          <option value="text">{$t('notify.format-text')}</option>
          <option value="json">{$t('notify.format-json')}</option>
        </NativeSelect>
      </Field>
    </section>

    <!-- A side effect (it sends something), so an explicit button, for every channel at once. -->
    <Button variant="outline" class="h-12 self-start" onclick={sendTest} disabled={busy}>{$t('notify.test')}</Button>
  </div>
  <Toaster />
</main>
