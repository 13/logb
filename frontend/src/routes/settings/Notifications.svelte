<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import { disablePush, enablePush, pushState, type PushState } from '../../lib/push';
  import type { NotificationSettings, NotificationTest } from '../../lib/types';

  let data = $state<NotificationSettings | null>(null);
  let push = $state<PushState | null>(null);
  let url = $state('');
  let format = $state<'text' | 'json'>('text');
  let busy = $state(false);
  let message = $state('');
  let error = $state('');
  let telegramLink = $state<string | null>(null);

  async function load() {
    data = await api<NotificationSettings>('GET', '/me/notifications');
  }

  onMount(async () => {
    try {
      await load();
      url = data?.url ?? '';
      format = data?.format ?? 'text';
    } catch (e) { error = (e as Error).message; }
    push = await pushState();
  });

  async function togglePush() {
    if (!data) return;
    busy = true; error = ''; message = '';
    try {
      push = push === 'on' ? await disablePush() : await enablePush(data.vapid_public_key);
      await load();
    } catch (e) { error = (e as Error).message; } finally { busy = false; }
  }

  async function saveWebhook() {
    busy = true; error = ''; message = '';
    try {
      data = await api<NotificationSettings>('PUT', '/me/notifications', { url: url.trim() || null, format });
      url = data.url ?? '';
      message = $t('object.saved');
    } catch (e) { error = (e as Error).message; } finally { busy = false; }
  }

  async function sendTest() {
    busy = true; error = ''; message = '';
    try {
      const r = await api<NotificationTest>('POST', '/me/notifications/test');
      const parts: string[] = [];
      if (r.push_sent + r.push_failed > 0) parts.push($t('notify.test-push', { n: r.push_sent }));
      if (r.webhook !== null) parts.push($t('notify.test-webhook', { result: r.webhook }));
      if (r.telegram !== null) parts.push($t('notify.test-telegram', { result: r.telegram }));
      message = parts.length > 0 ? parts.join(' ') : $t('notify.test-nowhere');
    } catch (e) { error = (e as Error).message; } finally { busy = false; }
  }

  async function linkTelegram() {
    busy = true; error = ''; message = '';
    try { telegramLink = (await api<{ url: string }>('POST', '/me/notifications/telegram/link')).url; }
    catch (e) { error = (e as Error).message; } finally { busy = false; }
  }

  async function unlinkTelegram() {
    busy = true; error = ''; message = '';
    try { await api('POST', '/me/notifications/telegram/unlink'); telegramLink = null; await load(); }
    catch (e) { error = (e as Error).message; } finally { busy = false; }
  }
</script>

<main>
  <TopBar title={$t('settings.notifications')} backTo="/settings" />
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if message}<p class="muted" role="status">{message}</p>{/if}

  <h2>{$t('notify.push-title')}</h2>
  {#if data}<p class="muted">{$t('notify.push-hint', { hour: data.hour })}</p>{/if}
  {#if push === 'unsupported'}
    <p class="hint">{$t('notify.push-unsupported')}</p>
  {:else if push === 'denied'}
    <p class="warn">{$t('notify.push-denied')}</p>
  {:else if push}
    {#if push === 'on'}<p>{$t('notify.push-on')}</p>{/if}
    <button class:primary={push !== 'on'} class:ghost={push === 'on'} onclick={togglePush} disabled={busy || !data}>
      {push === 'on' ? $t('notify.push-disable') : $t('notify.push-enable')}
    </button>
  {/if}
  {#if data && data.push_devices > 0}
    <p class="hint">{data.push_devices === 1 ? $t('notify.push-devices-one') : $t('notify.push-devices', { n: data.push_devices })}</p>
  {/if}

  <h2>{$t('notify.telegram-title')}</h2>
  {#if !data?.telegram_configured}
    <p class="muted">{$t('notify.telegram-unconfigured')}</p>
  {:else if data.telegram_connected}
    <p>{$t('notify.telegram-connected', { name: data.telegram_display_name ?? 'Telegram' })}</p>
    {#if data.telegram_last_error}<p class="error">{data.telegram_last_error}</p>{/if}
    <button class="ghost" onclick={unlinkTelegram} disabled={busy}>{$t('notify.telegram-disconnect')}</button>
  {:else}
    <p class="muted">{$t('notify.telegram-hint')}</p>
    <button class="primary" onclick={linkTelegram} disabled={busy}>{$t('notify.telegram-connect')}</button>
    {#if telegramLink}
      <p class="hint"><a href={telegramLink} target="_blank" rel="noreferrer">{$t('notify.telegram-open')}</a></p>
    {/if}
  {/if}

  <h2>{$t('notify.webhook-title')}</h2>
  <p class="muted">{data?.instance_webhook ? $t('notify.webhook-hint-instance') : $t('notify.webhook-hint')}</p>
  <div class="field">
    <label for="wu">{$t('notify.webhook-url')}</label>
    <input id="wu" type="url" inputmode="url" autocomplete="off" placeholder="https://ntfy.sh/…" bind:value={url} />
  </div>
  <div class="field">
    <label for="wf">{$t('notify.format')}</label>
    <select id="wf" bind:value={format}>
      <option value="text">{$t('notify.format-text')}</option>
      <option value="json">{$t('notify.format-json')}</option>
    </select>
  </div>
  <div class="list actions">
    <button onclick={saveWebhook} disabled={busy}>{$t('nav.save')}</button>
    <button class="ghost" onclick={sendTest} disabled={busy}>{$t('notify.test')}</button>
  </div>
</main>

<style>
  .actions { margin-top: var(--space-2); }
  .hint { font-size: var(--text-xs); color: var(--muted); margin-top: var(--space-2); }
</style>
