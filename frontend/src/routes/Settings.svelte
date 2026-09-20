<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import SettingsRow from '../lib/SettingsRow.svelte';
  import SignedIn from '../lib/SignedIn.svelte';
  import { locale } from '../i18n';
  import { api, deadOps, discardDeadOp, retryDead } from '../lib/api';
  import { t } from '../i18n';
  import { settings } from '../stores/settings';
  import { user } from '../stores/session';
  import { settingsRows } from '../lib/settings-rows';
  import type { ApiToken, DbDescription, NotificationSettings, User } from '../lib/types';
  import type { QueuedOp } from '../lib/outbox';

  let dead = $state<QueuedOp[]>([]);
  let tokenCount = $state<number | null>(null);
  let userCount = $state<number | null>(null);
  let backendLabel = $state<string | null>(null);
  /** The version the server reports, which can differ from this bundle's when a service worker
   *  is still serving the previous release. */
  let serverVersion = $state<string | null>(null);
  let notificationsLabel = $state<string | null>(null);

  const isAdmin = $derived($user?.is_admin === true);
  const built = $derived(
    new Intl.DateTimeFormat($locale, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(__BUILD_DATE__)),
  );

  /** Turns a count that may not have arrived yet into an already-translated, correctly
   *  pluralised label -- or `null` when there is nothing worth printing. Zero is folded into
   *  `null` here too: "no keys"/"no users" is the default state of every account, and a row
   *  that says so is noise on a screen meant to be scanned. */
  function countLabel(count: number | null, oneKey: string, manyKey: string): string | null {
    if (!count) return null;
    return count === 1 ? $t(oneKey) : $t(manyKey, { n: count });
  }

  /** The hub's rows, values and all. Everything here is a fact already in hand -- a count that
   *  has not arrived yet stays `null`, and its row simply shows nothing. */
  const rows = $derived(settingsRows({
    isAdmin,
    username: $user?.username ?? null,
    themeLabel: $t(`settings.theme-${$settings.theme}`),
    localeLabel: ($settings.locale === 'auto' ? navigator.language : $settings.locale).slice(0, 2).toUpperCase(),
    tokenLabel: countLabel(tokenCount, 'tokens.count-one', 'tokens.count'),
    userLabel: countLabel(userCount, 'settings.users-count-one', 'settings.users-count'),
    backendLabel,
    notificationsLabel,
  }));

  // Each of these fills in one row's value. They fail quietly: a hub whose Database row says
  // nothing is honest, while a hub that shows an error banner because a count did not load
  // makes a working instance look broken.
  onMount(async () => {
    dead = await deadOps();
    try { serverVersion = (await api<{ version: string }>('GET', '/health')).version; } catch { /* About shows nothing */ }
    try { tokenCount = (await api<ApiToken[]>('GET', '/auth/tokens')).length; } catch { /* row shows nothing */ }
    try {
      const n = await api<NotificationSettings>('GET', '/me/notifications');
      notificationsLabel = n.push_devices > 0
        ? countLabel(n.push_devices, 'notify.push-devices-one', 'notify.push-devices')
        : n.url ? $t('notify.webhook-title') : null;
    } catch { /* row shows nothing */ }
    if (!isAdmin) return;
    try { userCount = (await api<User[]>('GET', '/users')).length; } catch { /* row shows nothing */ }
    try {
      const db = await api<DbDescription>('GET', '/database');
      backendLabel = $t(db.backend === 'postgres' ? 'db.backend-postgres' : 'db.backend-sqlite');
    } catch { /* row shows nothing */ }
  });

  async function retryOutbox() {
    await retryDead();
    dead = await deadOps();
  }

  /** A permanently-rejected op (e.g. an upload naming an activity that will never exist) can
   *  never succeed no matter how many times "Try again" is pressed -- this is the only way to
   *  make it leave IndexedDB (and, for an upload, release its file bytes) short of the user
   *  clearing site data entirely. */
  async function discardOp(id: string) {
    if (!confirm($t('nav.confirm-delete'))) return;
    await discardDeadOp(id);
    dead = await deadOps();
  }
</script>

<main>
  <TopBar title={$t('settings.title')} backTo="/" />

  <!-- Not a row in a group: a failed write is an alert, it is the only time-sensitive thing on
       this screen, and it is absent entirely when the queue is clean. It expands here rather
       than behind a route of its own -- a URL for a screen that is almost always empty would be
       a seventh destination that exists to be blank. -->
  {#if dead.length > 0}
    <div class="banner">
      <b>{$t('outbox.failed')}</b>
    </div>
    <div class="list">
      {#each dead as op (op.id)}
        <div class="card row">
          <span>
            <b>{String(op.body.title || op.kind)}</b>
            <span class="muted">{op.path}</span>
            {#if op.lastError}<span class="error small">{op.lastError}</span>{/if}
          </span>
          <button class="ghost danger-text" onclick={() => discardOp(op.id)}>{$t('outbox.discard')}</button>
        </div>
      {/each}
    </div>
    <button onclick={retryOutbox}>{$t('outbox.retry')}</button>
  {/if}

  <!-- Who this is, and the way out, before anything else: on a phone this screen is the one tap
       from the tab bar that answers both. -->
  <div class="whoami"><SignedIn /></div>

  <h2>{$t('settings.you')}</h2>
  <div class="settings-grid">
    {#each rows.filter((r) => r.group === 'you') as row (row.id)}<SettingsRow {row} />{/each}
  </div>

  {#if isAdmin}
    <h2>{$t('settings.instance')}</h2>
    <div class="settings-grid">
      {#each rows.filter((r) => r.group === 'instance') as row (row.id)}<SettingsRow {row} />{/each}
    </div>
  {/if}

  <h2>{$t('settings.about')}</h2>
  <dl class="about card">
    <div><dt class="muted">{$t('settings.version')}</dt><dd class="tnum">{__APP_VERSION__}</dd></div>
    <div><dt class="muted">{$t('settings.built')}</dt><dd class="tnum">{built}</dd></div>
    {#if __BUILD_COMMIT__}<div><dt class="muted">{$t('settings.commit')}</dt><dd><code>{__BUILD_COMMIT__}</code></dd></div>{/if}
    {#if serverVersion}<div><dt class="muted">{$t('settings.server')}</dt><dd class="tnum">{serverVersion}{#if backendLabel} · {backendLabel}{/if}</dd></div>{/if}
  </dl>
  {#if serverVersion && serverVersion !== __APP_VERSION__}
    <p class="hint">{$t('settings.server-differs', { version: serverVersion })}</p>
  {/if}
</main>

<style>
  .row > button { flex: none; }
  .danger-text { color: var(--danger); }
  .whoami { margin-top: var(--space-2); }
  .about { display: grid; gap: var(--space-2); margin: 0; }
  .about div { display: flex; justify-content: space-between; gap: var(--space-3); }
  .about dd { margin: 0; text-align: right; overflow-wrap: anywhere; }
  .hint { font-size: var(--text-xs); color: var(--muted); margin-top: var(--space-2); }
</style>
