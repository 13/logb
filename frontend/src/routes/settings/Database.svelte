<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { api, ApiError } from '../../lib/api';
  import { locale, t } from '../../i18n';
  import { fmtDate } from '../../lib/format';
  import type { BackupStatus, DbDescription, DbLocation, DbProbe, DbSwitched } from '../../lib/types';

  let db = $state<DbDescription | null>(null);
  /** Who is backing this database up, from `GET /database/backup`. Null until it has answered,
   *  and null if it could not -- the section says nothing at all rather than guess, because a
   *  guess here is a guess about whether anything exists to restore from. */
  let backup = $state<BackupStatus | null>(null);
  /** What is typed into the connection-string field. It is sent, never stored and never read
   *  back: `GET /database` deliberately answers with a host and a database name and no URL, so
   *  there is nothing to prefill this with, and it is cleared the moment a switch succeeds so
   *  the password does not sit on the screen afterwards. */
  let dbUrl = $state('');
  let probe = $state<DbProbe | null>(null);
  let probing = $state(false);
  let switching = $state(false);
  let switched = $state<DbSwitched | null>(null);
  let dbError = $state('');
  let restarting = $state(false);
  let restartNote = $state('');

  /** The ceiling on the switch request. It has to sit far above any copy anybody will actually
   *  run: the server holds the connection open for the whole copy, and a client that gives up
   *  early does not stop it -- it only leaves the operator believing a migration failed that in
   *  fact succeeded, and about to run it a second time. */
  const SWITCH_TIMEOUT_MS = 30 * 60 * 1000;

  /** False when `LOGB_DATABASE_URL` is set, or the data directory will not take the pointer
   *  file. Either way a database chosen here could never be opened, so the field is read-only
   *  and the section says who decides instead. */
  const canChooseDb = $derived(db?.pointer_writable === true);

  onMount(async () => {
    await loadDatabase();
    await loadBackup();
  });

  async function loadDatabase() {
    try { db = await api<DbDescription>('GET', '/database'); } catch (e) { dbError = (e as Error).message; }
  }

  async function loadBackup() {
    try { backup = await api<BackupStatus>('GET', '/database/backup'); } catch (e) { dbError = (e as Error).message; }
  }

  /** The configured hour as a clock time. The server sends 0-23 in the instance's timezone, and
   *  "3" alone on a screen reads as a count of something rather than a time of day. */
  function hourText(hour: number | null): string {
    return hour === null ? '' : `${String(hour).padStart(2, '0')}:00`;
  }

  function backendName(at: DbLocation): string {
    return $t(at.backend === 'postgres' ? 'db.backend-postgres' : 'db.backend-sqlite');
  }

  /** A location as a line of text: the file for SQLite, `host / name` for PostgreSQL. Never a
   *  URL -- the server does not send one. */
  function place(at: DbLocation): string {
    if (at.backend === 'sqlite') return at.database ?? $t('db.memory');
    return [at.host, at.database].filter(Boolean).join(' / ');
  }

  async function testDatabase() {
    dbError = ''; probe = null; switched = null; probing = true;
    try { probe = await api<DbProbe>('POST', '/database/test', { url: dbUrl.trim() }); }
    catch (e) { dbError = (e as Error).message; }
    finally { probing = false; }
  }

  /** Runs for as long as the copy runs -- see `SWITCH_TIMEOUT_MS`. Nothing has moved when it
   *  returns: the instance is still serving the old database until it is restarted. */
  async function switchDatabase() {
    dbError = ''; probe = null; switched = null; switching = true;
    try {
      switched = await api<DbSwitched>('POST', '/database/switch', { url: dbUrl.trim() }, SWITCH_TIMEOUT_MS);
      dbUrl = '';
      await loadDatabase();
    } catch (e) {
      // A rejection from the server is a fact about the URL that was typed and is worth
      // showing. Anything else -- an aborted fetch, a proxy that closed the connection midway --
      // means this page does not know whether the copy finished, and saying "failed" would
      // invite a second migration on top of the first.
      dbError = e instanceof ApiError ? e.message : $t('db.switch-interrupted');
    } finally { switching = false; }
  }

  async function restartNow() {
    if (!confirm($t('db.restart-confirm'))) return;
    dbError = ''; restarting = true;
    try { await api('POST', '/database/restart'); restartNote = $t('db.restart-sent'); }
    catch (e) { dbError = (e as Error).message; restarting = false; }
  }
</script>

<main>
  <TopBar title={$t('db.title')} backTo="/settings" />
  {#if dbError}<p class="error">{dbError}</p>{/if}
  {#if db}
    <p class="muted">{$t('db.current')}</p>
    <div class="card stack">
      <b>{backendName(db)}</b>
      <span class="muted break">{place(db)}</span>
    </div>
    <!-- Above the field, not below it and not in a tooltip: a migrated instance looks
         perfectly healthy until somebody opens a photo, and by then the source machine may
         be gone. -->
    <div class="card blobs stack">
      <b>{$t('db.blobs-title')}</b>
      <span>{$t('db.blobs')}</span>
    </div>
    <div class="field">
      <label for="dburl">{$t('db.url')}</label>
      <!-- No placeholder and no hint about sending when the field cannot be used: an example
           URL in a read-only box reads like a value that is already saved. -->
      <input id="dburl" bind:value={dbUrl} placeholder={canChooseDb ? $t('db.url-placeholder') : ''} readonly={!canChooseDb}
             autocomplete="off" autocapitalize="off" spellcheck="false" />
      {#if canChooseDb}<span class="hint">{$t('db.url-hint')}</span>{/if}
    </div>
    {#if !canChooseDb}
      <p class="muted"><b>{$t('db.env')}</b> — {$t('db.env-hint')}</p>
    {:else}
      <div class="row">
        <button onclick={testDatabase} disabled={probing || switching || dbUrl.trim().length === 0}>
          {probing ? $t('db.testing') : $t('db.test')}
        </button>
        <button class="primary" onclick={switchDatabase} disabled={probing || switching || dbUrl.trim().length === 0}>
          {switching ? $t('db.switching') : $t('db.switch')}
        </button>
      </div>
      {#if switching}<p class="muted">{$t('db.switching-hint')}</p>{/if}
    {/if}
    {#if probe}
      <div class="card stack">
        <b>{probe.reachable ? $t('db.reachable') : $t('db.unreachable')}</b>
        {#if probe.version}<span class="muted break">{$t('db.version', { version: probe.version })}</span>{/if}
        {#if probe.state === 'empty'}<span class="muted">{$t('db.state-empty')}</span>{/if}
        {#if probe.state === 'holds_logb_data'}<span class="muted">{$t('db.state-holds')}</span>{/if}
        {#if probe.message}<span class="muted break">{probe.message}</span>{/if}
      </div>
    {/if}
    {#if switched}
      <div class="card stack">
        <b>{$t('db.switched')}</b>
        <ul class="tables">
          {#each switched.tables as tb (tb.table)}
            <li><span>{tb.table}</span><span class="muted">{$t('db.rows', { rows: tb.rows })}</span></li>
          {/each}
        </ul>
        <span class="muted break">{$t('db.epoch', { epoch: switched.epoch })}</span>
        <span class="muted break">{$t('db.pointer', { path: switched.pointer })}</span>
      </div>
    {/if}
    {#if db.pending || switched}
      <div class="card restart stack">
        <b>{$t('db.pending')}</b>
        {#if db.pending}<span class="muted break">{$t('db.pending-at', { where: `${backendName(db.pending)} · ${place(db.pending)}` })}</span>{/if}
        <!-- Nothing here can check that a supervisor exists, so the button does not promise
             one. "Restart now" that quietly means "stop now" is how an instance ends up down
             overnight. -->
        <span>{$t('db.restart-hint')}</span>
        <button class="danger" onclick={restartNow} disabled={restarting}>{$t('db.restart')}</button>
        {#if restartNote}<span class="muted">{restartNote}</span>{/if}
      </div>
    {/if}
  {/if}

  <h2>{$t('backup.title')}</h2>
  <!-- Three states, one of which is the reason this section exists: on PostgreSQL LogB backs
       up nothing, and an instance migrated from SQLite through the section just above looks
       in every other way as if its nightly backups came along with it. That is said as a
       division of responsibility rather than as an error, because it is not a fault -- but it
       is said plainly enough that nobody reads this screen and still believes otherwise. -->
  {#if backup}
    <div class="card stack" class:elsewhere={backup.state === 'not_ours'}>
      {#if backup.state === 'scheduled'}
        <b>{$t('backup.scheduled-title')}</b>
        <span class="break">{$t('backup.scheduled', { directory: backup.directory ?? '', hour: hourText(backup.hour) })}</span>
        <span class="muted">
          {backup.last_at
            ? $t('backup.last', { date: fmtDate(backup.last_at, $locale) })
            : $t('backup.last-none', { hour: hourText(backup.hour) })}
        </span>
      {:else if backup.state === 'off'}
        <b>{$t('backup.off-title')}</b>
        <span>{$t('backup.off')}</span>
      {:else}
        <b>{$t('backup.not-ours-title')}</b>
        <span>{$t('backup.not-ours')}</span>
        <span>{$t('backup.not-ours-how')}</span>
      {/if}
    </div>
  {/if}
</main>

<style>
  .row > button { flex: none; }
  .stack { display: grid; gap: var(--space-2); margin-bottom: var(--space-3); }
  /* A host, a file path, an epoch and a driver's error message are all long and none of them
     may be cut off half way through. */
  .break { word-break: break-word; }
  /* The one thing on this screen a migration can silently lose. It is coloured and bordered so
     it cannot be read as another hint under another field. */
  .blobs { border-left: 4px solid var(--warn); }
  .blobs b { color: var(--warn); }
  .restart { border-left: 4px solid var(--danger); }
  /* Marked out, but in the accent colour rather than the warning or danger one: a PostgreSQL
     database LogB does not back up is a division of responsibility, not a fault. */
  .elsewhere { border-left: 4px solid var(--accent); }
  .elsewhere b { color: var(--accent); }
  .tables { list-style: none; padding: 0; margin: 0; display: grid; gap: var(--space-1); }
  .tables li { display: flex; justify-content: space-between; gap: var(--space-2); font-size: var(--text-sm); }
</style>
