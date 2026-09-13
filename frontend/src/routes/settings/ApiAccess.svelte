<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import { api } from '../../lib/api';
  import { locale, t } from '../../i18n';
  import { fmtDate } from '../../lib/format';
  import type { ApiToken } from '../../lib/types';

  let tokens = $state<ApiToken[]>([]);
  let tokenName = $state('');
  /** The plaintext of a token just created. The server returns it once and stores only a hash,
   *  so this is the single moment it can be read -- it is deliberately not persisted anywhere,
   *  and is dropped as soon as the user creates another or leaves the screen. */
  let freshToken = $state('');
  let copied = $state(false);
  let error = $state('');

  onMount(async () => {
    await loadTokens();
  });

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
</script>

<main>
  <TopBar title={$t('tokens.title')} backTo="/settings" />
  {#if error}<p class="error">{error}</p>{/if}

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
</main>

<style>
  .fresh-token { display: grid; gap: var(--space-2); }
  /* The token is long and must be readable in full, since it can never be shown again. */
  .fresh-token code { word-break: break-all; font-size: var(--text-sm); background: var(--surface-2); padding: var(--space-2); border-radius: var(--radius-sm); }
  .row > button { flex: none; }
  .danger-text { color: var(--danger); }
</style>
