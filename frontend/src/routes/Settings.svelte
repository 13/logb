<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import SettingsRow from '../lib/SettingsRow.svelte';
  import { api, deadOps, discardDeadOp, retryDead } from '../lib/api';
  import { t } from '../i18n';
  import { settings } from '../stores/settings';
  import { user } from '../stores/session';
  import { settingsRows } from '../lib/settings-rows';
  import type { ApiToken, DbDescription, User } from '../lib/types';
  import type { QueuedOp } from '../lib/outbox';

  let dead = $state<QueuedOp[]>([]);
  let tokenCount = $state<number | null>(null);
  let userCount = $state<number | null>(null);
  let backendLabel = $state<string | null>(null);
  let error = $state('');

  const isAdmin = $derived($user?.is_admin === true);

  /** The hub's rows, values and all. Everything here is a fact already in hand -- a count that
   *  has not arrived yet stays `null`, and its row simply shows nothing. */
  const rows = $derived(settingsRows({
    isAdmin,
    username: $user?.username ?? null,
    themeLabel: $t(`settings.theme-${$settings.theme}`),
    localeLabel: ($settings.locale === 'auto' ? navigator.language : $settings.locale).slice(0, 2).toUpperCase(),
    tokenCount, userCount, backendLabel,
  }));

  // Each of these fills in one row's value. They fail quietly: a hub whose Database row says
  // nothing is honest, while a hub that shows an error banner because a count did not load
  // makes a working instance look broken.
  onMount(async () => {
    dead = await deadOps();
    try { tokenCount = (await api<ApiToken[]>('GET', '/auth/tokens')).length; } catch { /* row shows nothing */ }
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
  {#if error}<p class="error">{error}</p>{/if}

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
            <b>{String(op.body.title ?? op.kind)}</b>
            <span class="muted">{op.path}</span>
          </span>
          <button class="ghost danger-text" onclick={() => discardOp(op.id)}>{$t('outbox.discard')}</button>
        </div>
      {/each}
    </div>
    <button onclick={retryOutbox}>{$t('outbox.retry')}</button>
  {/if}

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
</main>

<style>
  .danger-text { color: var(--danger); }
</style>
