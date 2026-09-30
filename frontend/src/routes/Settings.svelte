<script lang="ts">
  import { dateTimeFormat } from '../lib/intl-cache';
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
  import { describeFailedWrite } from '../lib/failed-write';
  import { Button } from '$lib/components/ui/button/index.js';
  import { destructiveGhostClass, hintClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';

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
    dateTimeFormat($locale, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(__BUILD_DATE__)),
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
  //
  // All at once: none of them depends on another, and one after the other the hub took seven
  // round trips to fill in. Each keeps its own quiet failure.
  onMount(() => {
    void Promise.allSettled([
      deadOps().then((ops) => { dead = ops; }),
      api<{ version: string }>('GET', '/health').then((h) => { serverVersion = h.version; }),
      api<ApiToken[]>('GET', '/auth/tokens').then((list) => { tokenCount = list.length; }),
      api<NotificationSettings>('GET', '/me/notifications').then((n) => {
        notificationsLabel = n.push_devices > 0
          ? countLabel(n.push_devices, 'notify.push-devices-one', 'notify.push-devices')
          : n.url ? $t('notify.webhook-title') : null;
      }),
      ...(isAdmin
        ? [
            api<User[]>('GET', '/users').then((list) => { userCount = list.length; }),
            api<DbDescription>('GET', '/database').then((db) => {
              backendLabel = $t(db.backend === 'postgres' ? 'db.backend-postgres' : 'db.backend-sqlite');
            }),
          ]
        : []),
    ]);
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

  const you = $derived(rows.filter((r) => r.group === 'you'));
  const instance = $derived(rows.filter((r) => r.group === 'instance'));
  const group = 'm-0 list-none divide-y divide-border overflow-hidden rounded-lg border border-border bg-card p-0 shadow-xs';
  const pair = 'flex justify-between gap-3';
  const value = 'm-0 text-right text-foreground tabular-nums [overflow-wrap:anywhere]';
</script>

<main>
  <TopBar title={$t('settings.title')} backTo="/" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    <!-- A failed write is an alert and the only time-sensitive thing here, absent when the queue is
         clean. It expands in place rather than behind a route of its own. -->
    {#if dead.length > 0}
      <section aria-labelledby="settings-failed" class="flex flex-col gap-3 rounded-lg border border-destructive/30 bg-destructive/10 p-3">
        <h2 id="settings-failed" class="m-0 text-base font-semibold text-destructive">{$t('outbox.failed')}</h2>
        <ul role="list" class="m-0 flex list-none flex-col gap-2 p-0">
          {#each dead as op (op.id)}
            {@const d = describeFailedWrite(op, $t)}
            <li data-testid="failed-write" class="flex items-center gap-3 rounded-md bg-card p-3">
              <span class="flex min-w-0 flex-1 flex-col gap-1 [overflow-wrap:anywhere]">
                <b class="font-semibold text-foreground">{d.what}{#if d.name}: {d.name}{/if}</b>
                {#if d.reason}<span class="text-sm text-destructive">{d.reason}</span>{/if}
              </span>
              <Button variant="ghost" class={['min-h-11 shrink-0', destructiveGhostClass]} onclick={() => discardOp(op.id)}>{$t('outbox.discard')}</Button>
            </li>
          {/each}
        </ul>
        <Button class="h-12 self-start" onclick={retryOutbox}>{$t('outbox.retry')}</Button>
      </section>
    {/if}

    <!-- Who this is, and the way out, first: on a phone this screen is one tap from the tab bar. -->
    <SignedIn />

    <section class="flex flex-col gap-2">
      <h2 id="settings-you" class={sectionHeadingClass}>{$t('settings.you')}</h2>
      <ul role="list" aria-labelledby="settings-you" class={group}>
        {#each you as row (row.id)}<li><SettingsRow {row} /></li>{/each}
      </ul>
    </section>

    {#if isAdmin}
      <section class="flex flex-col gap-2">
        <h2 id="settings-instance" class={sectionHeadingClass}>{$t('settings.instance')}</h2>
        <ul role="list" aria-labelledby="settings-instance" class={group}>
          {#each instance as row (row.id)}<li><SettingsRow {row} /></li>{/each}
        </ul>
      </section>
    {/if}

    <section class="flex flex-col gap-2">
      <h2 id="settings-about" class={sectionHeadingClass}>{$t('settings.about')}</h2>
      <dl data-testid="about" aria-labelledby="settings-about" class="m-0 flex flex-col gap-2 rounded-lg border border-border bg-card p-4 text-sm shadow-xs">
        <div class={pair}><dt class="text-muted-foreground">{$t('settings.version')}</dt><dd class={value}>{__APP_VERSION__}</dd></div>
        <div class={pair}><dt class="text-muted-foreground">{$t('settings.built')}</dt><dd class={value}>{built}</dd></div>
        {#if __BUILD_COMMIT__}<div class={pair}><dt class="text-muted-foreground">{$t('settings.commit')}</dt><dd class={value}><code class="font-mono text-xs">{__BUILD_COMMIT__}</code></dd></div>{/if}
        {#if serverVersion}<div class={pair}><dt class="text-muted-foreground">{$t('settings.server')}</dt><dd class={value}>{serverVersion}{#if backendLabel}{` · ${backendLabel}`}{/if}</dd></div>{/if}
      </dl>
      {#if serverVersion && serverVersion !== __APP_VERSION__}
        <p class={hintClass}>{$t('settings.server-differs', { version: serverVersion })}</p>
      {/if}
    </section>
  </div>
</main>
