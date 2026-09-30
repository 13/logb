<script lang="ts">
  import { errorMessage } from '../../lib/api-error';
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { api, ApiError } from '../../lib/api';
  import { t } from '../../i18n';
  import { fmtDate } from '../../lib/format';
  import { dateFormat } from '../../stores/date-format';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { errorClass, hintClass } from '$lib/components/ui/field/classes.js';
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
    try { db = await api<DbDescription>('GET', '/database'); } catch (e) { dbError = errorMessage(e, $t); }
  }

  async function loadBackup() {
    try { backup = await api<BackupStatus>('GET', '/database/backup'); } catch (e) { dbError = errorMessage(e, $t); }
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
    catch (e) { dbError = errorMessage(e, $t); }
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
      dbError = e instanceof ApiError ? errorMessage(e, $t) : $t('db.switch-interrupted');
    } finally { switching = false; }
  }

  async function restartNow() {
    if (!confirm($t('db.restart-confirm'))) return;
    dbError = ''; restarting = true;
    try { await api('POST', '/database/restart'); restartNote = $t('db.restart-sent'); }
    catch (e) { dbError = errorMessage(e, $t); restarting = false; }
  }

  const card = 'flex flex-col gap-3 rounded-lg border border-border bg-card p-4 shadow-xs';
  const heading = 'm-0 text-base font-semibold text-foreground';
  /** A host, a file path, an epoch or a driver's message: long, and never cut off. */
  const long = 'm-0 text-sm text-muted-foreground [overflow-wrap:anywhere]';
</script>

<main>
  <TopBar title={$t('db.title')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6" aria-busy={db === null}>
    {#if dbError}<p role="alert" class={errorClass}>{dbError}</p>{/if}
    {#if db}
      <section aria-labelledby="db-current" class={card}>
        <h2 id="db-current" class={heading}>{$t('db.current')}</h2>
        <p class="m-0 font-semibold text-foreground">{backendName(db)}</p>
        <p class={long}>{place(db)}</p>
      </section>

      <!-- Above the field, not below it and not in a tooltip: a migrated instance looks healthy
           until somebody opens a photo, and by then the source machine may be gone. Marked in the
           warning colour so it cannot be read as another hint. -->
      <div class={`${card} border-l-4 border-l-warn`}>
        <p class="m-0 font-semibold text-warn">{$t('db.blobs-title')}</p>
        <p class="m-0 text-sm text-foreground">{$t('db.blobs')}</p>
      </div>

      <div class={card}>
        <!-- No placeholder and no hint when the field cannot be used: an example URL in a
             read-only box reads like a value that is already saved. -->
        <Field id="dburl" label={$t('db.url')} hint={canChooseDb ? $t('db.url-hint') : ''}>
          <Input bind:value={dbUrl} placeholder={canChooseDb ? $t('db.url-placeholder') : ''} readonly={!canChooseDb}
                 autocomplete="off" autocapitalize="off" spellcheck="false" />
        </Field>
        {#if !canChooseDb}
          <p class="m-0 text-sm text-muted-foreground"><b class="font-semibold text-foreground">{$t('db.env')}</b> — {$t('db.env-hint')}</p>
        {:else}
          <div class="flex flex-wrap gap-2">
            <Button variant="outline" class="h-12" onclick={testDatabase} disabled={probing || switching || dbUrl.trim().length === 0}>
              {probing ? $t('db.testing') : $t('db.test')}
            </Button>
            <!-- The page's one primary action. -->
            <Button class="h-12" onclick={switchDatabase} disabled={probing || switching || dbUrl.trim().length === 0}>
              {switching ? $t('db.switching') : $t('db.switch')}
            </Button>
          </div>
          {#if switching}<p class={hintClass}>{$t('db.switching-hint')}</p>{/if}
        {/if}
      </div>

      {#if probe}
        <div class={card}>
          <p class="m-0 font-semibold text-foreground">{probe.reachable ? $t('db.reachable') : $t('db.unreachable')}</p>
          {#if probe.version}<p class={long}>{$t('db.version', { version: probe.version })}</p>{/if}
          {#if probe.state === 'empty'}<p class={long}>{$t('db.state-empty')}</p>{/if}
          {#if probe.state === 'holds_logb_data'}<p class={long}>{$t('db.state-holds')}</p>{/if}
          {#if probe.message}<p class={long}>{probe.message}</p>{/if}
        </div>
      {/if}
      {#if switched}
        <div class={card}>
          <p class="m-0 font-semibold text-foreground">{$t('db.switched')}</p>
          <ul role="list" class="m-0 flex list-none flex-col gap-1 p-0 text-sm">
            {#each switched.tables as tb (tb.table)}
              <li class="flex justify-between gap-2"><span class="text-foreground">{tb.table}</span><span class="text-muted-foreground tabular-nums">{$t('db.rows', { rows: tb.rows })}</span></li>
            {/each}
          </ul>
          <p class={long}>{$t('db.epoch', { epoch: switched.epoch })}</p>
          <p class={long}>{$t('db.pointer', { path: switched.pointer })}</p>
        </div>
      {/if}
      {#if db.pending || switched}
        <div class={`${card} border-l-4 border-l-destructive`}>
          <p class="m-0 font-semibold text-foreground">{$t('db.pending')}</p>
          {#if db.pending}<p class={long}>{$t('db.pending-at', { where: `${backendName(db.pending)} · ${place(db.pending)}` })}</p>{/if}
          <!-- Nothing here can check that a supervisor exists, so the button does not promise one. -->
          <p class="m-0 text-sm text-foreground">{$t('db.restart-hint')}</p>
          <Button variant="destructive" class="h-12 self-start" onclick={restartNow} disabled={restarting}>{$t('db.restart')}</Button>
          {#if restartNote}<p class={hintClass}>{restartNote}</p>{/if}
        </div>
      {/if}
    {/if}

    <section aria-labelledby="backup-title" class="flex flex-col gap-2">
      <h2 id="backup-title" class={heading}>{$t('backup.title')}</h2>
      <!-- Three states; on PostgreSQL LogB backs up nothing, said as a division of responsibility
           (accent ink), not as a fault. -->
      {#if backup}
        <div class={[card, backup.state === 'not_ours' && 'border-l-4 border-l-brand-ink']}>
          {#if backup.state === 'scheduled' || backup.state === 'stale'}
            <p class="m-0 font-semibold text-foreground">{$t('backup.scheduled-title')}</p>
            <p class="m-0 text-sm text-foreground [overflow-wrap:anywhere]">{$t('backup.scheduled', { directory: backup.directory ?? '', hour: hourText(backup.hour) })}</p>
            <p class={hintClass}>
              {backup.last_at ? $t('backup.last', { date: fmtDate(backup.last_at, $dateFormat) }) : $t('backup.last-none', { hour: hourText(backup.hour) })}
            </p>
            {#if backup.state === 'stale'}<p class={errorClass}>{$t('backup.stale')}</p>{/if}
          {:else if backup.state === 'off'}
            <p class="m-0 font-semibold text-foreground">{$t('backup.off-title')}</p>
            <p class="m-0 text-sm text-foreground">{$t('backup.off')}</p>
          {:else}
            <p class="m-0 font-semibold text-brand-ink">{$t('backup.not-ours-title')}</p>
            <p class="m-0 text-sm text-foreground">{$t('backup.not-ours')}</p>
            <p class="m-0 text-sm text-foreground">{$t('backup.not-ours-how')}</p>
          {/if}
        </div>
      {/if}
    </section>
  </div>
</main>
