<script lang="ts">
  import { go } from './router';
  import { fileUrl } from './api';
  import { counter, money } from './format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { MemObject } from './types';
  let { object }: { object: MemObject } = $props();
</script>

<div class="card-row">
  <button class="card list-card" onclick={() => go(`/objects/${object.id}`)}>
    {#if object.cover_file_id}
      <img class="thumb cover" src={fileUrl(object.cover_file_id, true)} alt="" loading="lazy" />
    {/if}
    <div class="body">
      <div class="row">
        <b>{object.name}</b>
        {#if object.archived_at}<span class="chip">{$t('dash.archived')}</span>{/if}
        {#if object.stats.due_reminder_count > 0}
          <span class="chip due">{object.stats.due_reminder_count === 1 ? $t('dash.due-one') : $t('dash.due', { n: object.stats.due_reminder_count })}</span>
        {/if}
      </div>
      <div class="muted tnum">
        {object.category}
        {#if object.stats.current_counter !== null} · {counter(object.stats.current_counter, object.counter_unit, $locale)}{/if}
        {#if object.stats.total_cost_cents > 0} · {money(object.stats.total_cost_cents, $currency, $locale)}{/if}
      </div>
    </div>
  </button>
  <button class="ghost quicklog" aria-label={$t('dash.log')}
          onclick={() => go(`/objects/${object.id}/activities/new`)}>＋</button>
</div>

<style>
  .card-row { display: flex; gap: 8px; align-items: stretch; }
  .card-row > .list-card { flex: 1; min-width: 0; }
  .quicklog { flex: none; width: 48px; font-size: 1.4rem; border: 1px solid var(--border); border-radius: var(--radius-md); }
  .list-card { display: flex; gap: 12px; align-items: center; text-align: left; width: 100%; background: var(--surface); border: 1px solid var(--border); }
  .cover { width: 64px; height: 64px; flex: none; }
  .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .row > b { flex: none; }
</style>
