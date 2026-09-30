<script lang="ts">
  import { errorMessage } from '../../lib/api-error';
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { api } from '../../lib/api';
  import { t } from '../../i18n';
  import { Button } from '$lib/components/ui/button/index.js';
  import { Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { errorClass, hintClass, destructiveGhostClass } from '$lib/components/ui/field/classes.js';
  import { fmtDate } from '../../lib/format';
  import { dateFormat } from '../../stores/date-format';
  import type { ApiToken } from '../../lib/types';

  let tokens = $state<ApiToken[]>([]);
  let tokenName = $state('');
  /** The plaintext of a token just created. The server returns it once and stores only a hash,
   *  so this is the single moment it can be read -- it is deliberately not persisted anywhere,
   *  and is dropped as soon as the user creates another or leaves the screen. */
  let freshToken = $state('');
  let copied = $state(false);
  let error = $state('');

  const card = 'flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs';

  onMount(async () => {
    await loadTokens();
  });

  async function loadTokens() {
    try { tokens = await api<ApiToken[]>('GET', '/auth/tokens'); } catch (e) { error = errorMessage(e, $t); }
  }

  async function createToken(e: SubmitEvent) {
    e.preventDefault();
    error = ''; copied = false;
    try {
      const made = await api<ApiToken & { token: string }>('POST', '/auth/tokens', { name: tokenName.trim() });
      freshToken = made.token;
      tokenName = '';
      await loadTokens();
    } catch (e) { error = errorMessage(e, $t); }
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
    } catch (e) { error = errorMessage(e, $t); }
  }
</script>

<main>
  <TopBar title={$t('tokens.title')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
    <p class={hintClass}>{$t('tokens.intro')}</p>

    {#if freshToken}
      <div class="flex flex-col gap-2 rounded-lg border border-brand-ink bg-primary/10 p-4">
        <p class="m-0 text-sm font-medium text-foreground">{$t('tokens.created')}</p>
        <!-- Long, and never shown again: readable in full. -->
        <code data-testid="fresh-token" class="block rounded-md bg-card p-2 font-mono text-sm break-all text-foreground">{freshToken}</code>
        <Button variant="outline" class="min-h-11 self-start" onclick={copyToken}>{copied ? $t('tokens.copied') : $t('tokens.copy')}</Button>
      </div>
    {/if}

    <ul role="list" data-testid="token-list" class="m-0 flex list-none flex-col gap-2 p-0">
      {#each tokens as tok (tok.id)}
        <li class="flex min-h-14 items-center gap-3 rounded-lg border border-border bg-card px-3 py-2 shadow-xs">
          <span class="flex min-w-0 flex-1 flex-col">
            <span class="truncate font-medium text-foreground">{tok.name}</span>
            <span class="truncate text-sm text-muted-foreground tabular-nums">
              {tok.prefix}… · {tok.last_used_at ? $t('tokens.last-used', { date: fmtDate(tok.last_used_at.slice(0, 10), $dateFormat) }) : $t('tokens.never-used')}
            </span>
          </span>
          <Button variant="ghost" class={`min-h-11 shrink-0 ${destructiveGhostClass}`} onclick={() => revokeToken(tok)}>{$t('tokens.revoke')}</Button>
        </li>
      {:else}
        <li class="text-sm text-muted-foreground">{$t('tokens.none')}</li>
      {/each}
    </ul>

    <form onsubmit={createToken} class={`m-0 ${card}`}>
      <Field id="tn" label={$t('tokens.name')}>
        <Input bind:value={tokenName} placeholder={$t('tokens.name-placeholder')} maxlength={64} autocomplete="off" />
      </Field>
      <!-- The page's one primary action. -->
      <Button type="submit" class="h-12 self-start" disabled={tokenName.trim().length === 0}>{$t('tokens.create')}</Button>
      <p class={hintClass}>{$t('tokens.password-note')}</p>
    </form>
  </div>
</main>
