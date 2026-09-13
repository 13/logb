<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import { api, ApiError, deadOps, discardDeadOp, retryDead, uploadRaw } from '../lib/api';
  import { locale, t } from '../i18n';
  import { LANG_NAMES, SUPPORTED } from '../i18n/detect';
  import { fmtDate } from '../lib/format';
  import { settings } from '../stores/settings';
  import { currency, user, logout, logoutEverywhere } from '../stores/session';
  import type { ApiToken, BackupStatus, DbDescription, DbLocation, DbProbe, DbSwitched, ImportCounts, User } from '../lib/types';
  import type { QueuedOp } from '../lib/outbox';

  let dead = $state<QueuedOp[]>([]);
  let users = $state<User[]>([]);
  let newName = $state('');
  let newPass = $state('');
  let newAdmin = $state(false);
  let ownPass = $state('');
  let currencyText = $state('');
  let message = $state('');
  let error = $state('');
  let fileEl: HTMLInputElement;
  let tokens = $state<ApiToken[]>([]);
  let tokenName = $state('');
  /** The plaintext of a token just created. The server returns it once and stores only a hash,
   *  so this is the single moment it can be read -- it is deliberately not persisted anywhere,
   *  and is dropped as soon as the user creates another or leaves the screen. */
  let freshToken = $state('');
  let copied = $state(false);

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

  const isAdmin = $derived($user?.is_admin === true);
  /** False when `LOGB_DATABASE_URL` is set, or the data directory will not take the pointer
   *  file. Either way a database chosen here could never be opened, so the field is read-only
   *  and the section says who decides instead. */
  const canChooseDb = $derived(db?.pointer_writable === true);

  onMount(async () => {
    currencyText = $currency;
    dead = await deadOps();
    await loadTokens();
    if (isAdmin) { await loadUsers(); await loadDatabase(); await loadBackup(); }
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

  async function loadTokens() {
    try { tokens = await api<ApiToken[]>('GET', '/auth/tokens'); } catch (e) { error = (e as Error).message; }
  }

  async function createToken() {
    error = ''; copied = false;
    try {
      const made = await api<ApiToken & { token: string }>('POST', '/auth/tokens', { name: tokenName.trim() });
      freshToken = made.token;
      tokenName = '';
      await loadTokens();
    } catch (e) { error = (e as Error).message; }
  }

  async function copyToken() {
    try {
      await navigator.clipboard.writeText(freshToken);
      copied = true;
    } catch {
      // No clipboard permission, or an insecure origin: the value is on screen to be selected
      // by hand, so this is a convenience failing, not the feature failing.
    }
  }

  async function revokeToken(tok: ApiToken) {
    if (!confirm($t('tokens.revoke-confirm'))) return;
    try {
      await api('DELETE', `/auth/tokens/${tok.id}`);
      await loadTokens();
    } catch (e) { error = (e as Error).message; }
  }

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

  async function loadUsers() {
    try { users = await api<User[]>('GET', '/users'); } catch (e) { error = (e as Error).message; }
  }

  async function saveCurrency() {
    try {
      const s = await api<{ currency: string }>('PUT', '/settings', { currency: currencyText.trim().toUpperCase() });
      currency.set(s.currency);
      message = $t('object.saved');
    } catch (e) { error = (e as Error).message; }
  }

  async function addUser() {
    try {
      await api('POST', '/users', { username: newName, password: newPass, is_admin: newAdmin });
      newName = ''; newPass = ''; newAdmin = false;
      await loadUsers();
    } catch (e) { error = (e as Error).message; }
  }

  async function removeUser(u: User) {
    if (!confirm($t('nav.confirm-delete'))) return;
    try { await api('DELETE', `/users/${u.id}`); await loadUsers(); } catch (e) { error = (e as Error).message; }
  }

  async function signOutEverywhere() {
    if (!confirm($t('settings.logout-all-confirm'))) return;
    try { await logoutEverywhere(); } catch (e) { error = (e as Error).message; }
  }

  async function changeOwnPassword() {
    if (!$user) return;
    try {
      await api('PATCH', `/users/${$user.id}`, { password: ownPass });
      ownPass = ''; message = $t('object.saved');
    } catch (e) { error = (e as Error).message; }
  }

  async function doImport(files: FileList | null) {
    if (!files || files.length === 0) return;
    try {
      const counts = await uploadRaw<ImportCounts>('/import', files[0], 'application/zip');
      message = $t('settings.import-done', counts as unknown as Record<string, number>);
    } catch (e) { error = (e as Error).message; } finally { fileEl.value = ''; }
  }
</script>

<main>
  <TopBar title={$t('settings.title')} backTo="/" />
  {#if error}<p class="error">{error}</p>{/if}
  {#if message}<p class="muted">{message}</p>{/if}

  {#if dead.length > 0}
    <h2>{$t('outbox.failed')}</h2>
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

  <h2>{$t('settings.account')}</h2>
  <p class="muted">{$user?.username}</p>
  <div class="row">
    <div class="field"><label for="op">{$t('settings.change-password')}</label><input id="op" type="password" bind:value={ownPass} autocomplete="new-password" /></div>
    <button onclick={changeOwnPassword} disabled={ownPass.length < 8}>{$t('nav.save')}</button>
  </div>
  <button class="ghost" onclick={logout}>{$t('login.logout')}</button>
  <button class="ghost" onclick={signOutEverywhere}>{$t('settings.logout-all')}</button>
  <p class="muted hint">{$t('settings.logout-all-hint')}</p>

  {#if isAdmin}
    <h2>{$t('settings.currency')}</h2>
    <div class="row">
      <div class="field"><input bind:value={currencyText} maxlength="3" aria-label={$t('settings.currency')} /><span class="hint">{$t('settings.currency-hint')}</span></div>
      <button onclick={saveCurrency}>{$t('nav.save')}</button>
    </div>

    <h2>{$t('settings.users')}</h2>
    <div class="list">
      {#each users as u (u.id)}
        <div class="card row">
          <span>{u.username}{#if u.is_admin} · {$t('settings.user-admin')}{/if}</span>
          {#if u.id !== $user?.id}<button class="ghost danger-text" onclick={() => removeUser(u)}>{$t('settings.user-delete')}</button>{/if}
        </div>
      {/each}
    </div>
    <h2>{$t('settings.user-new')}</h2>
    <div class="field"><label for="nu">{$t('login.username')}</label><input id="nu" bind:value={newName} /></div>
    <div class="field"><label for="np">{$t('login.password')}</label><input id="np" type="password" bind:value={newPass} autocomplete="new-password" /></div>
    <label class="row toggle"><input type="checkbox" bind:checked={newAdmin} /> {$t('settings.user-admin')}</label>
    <button class="primary" onclick={addUser} disabled={newName.length < 3 || newPass.length < 8}>{$t('settings.user-new')}</button>

    <h2>{$t('db.title')}</h2>
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
  {/if}

  <h2>{$t('tokens.title')}</h2>
  <p class="muted">{$t('tokens.intro')}</p>
  {#if freshToken}
    <div class="card fresh-token">
      <p>{$t('tokens.created')}</p>
      <code>{freshToken}</code>
      <button onclick={copyToken}>{copied ? $t('tokens.copied') : $t('tokens.copy')}</button>
    </div>
  {/if}
  <div class="list">
    {#each tokens as tok (tok.id)}
      <div class="card row">
        <span>
          {tok.name}
          <span class="muted small">
            {tok.prefix}… ·
            {tok.last_used_at ? $t('tokens.last-used', { date: fmtDate(tok.last_used_at.slice(0, 10), $locale) }) : $t('tokens.never-used')}
          </span>
        </span>
        <button class="ghost danger-text" onclick={() => revokeToken(tok)}>{$t('tokens.revoke')}</button>
      </div>
    {:else}
      <p class="muted">{$t('tokens.none')}</p>
    {/each}
  </div>
  <div class="field">
    <label for="tn">{$t('tokens.name')}</label>
    <input id="tn" bind:value={tokenName} placeholder={$t('tokens.name-placeholder')} maxlength="64" />
  </div>
  <button class="primary" onclick={createToken} disabled={tokenName.trim().length === 0}>{$t('tokens.create')}</button>
  <p class="muted small">{$t('tokens.password-note')}</p>

  <h2>{$t('settings.data')}</h2>
  <div class="list">
    <a class="button-like" href="/api/export">{$t('settings.export')}</a>
    <button onclick={() => fileEl.click()}>{$t('settings.import')}</button>
    <input bind:this={fileEl} type="file" accept=".zip,application/zip" hidden onchange={(e) => doImport((e.currentTarget as HTMLInputElement).files)} />
  </div>
  <!-- Beside the button, not in the Backup section: the export is the thing somebody reaches
       for when they mean "keep a copy", and it is genuinely useful -- just not a backup of the
       database. -->
  <p class="muted">{$t('settings.export-not-backup')}</p>
</main>

<style>
  .fresh-token { display: grid; gap: var(--space-2); }
  /* The token is long and must be readable in full, since it can never be shown again. */
  .fresh-token code { word-break: break-all; font-size: var(--text-sm); background: var(--surface-2); padding: var(--space-2); border-radius: var(--radius-sm); }
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
  .toggle input { flex: none; width: 20px; height: 20px; }
  .danger-text { color: var(--danger); }
</style>
