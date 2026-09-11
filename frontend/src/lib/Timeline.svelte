<script lang="ts">
  import { fileUrl } from './api';
  import { go } from './router';
  import { counter, fmtDate, money } from './format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import { groupByYear } from './activity-form';
  import { CATEGORIES, type Activity, type Category, type CounterUnit } from './types';

  let { objectId, activities, total, loadingMore = false, onmore, unit, category = $bindable('') }:
    {
      objectId: number; activities: Activity[]; total: number; loadingMore?: boolean;
      onmore?: () => void; unit: CounterUnit; category?: Category | '';
    } = $props();
  const groups = $derived(groupByYear(activities));
  const hasMore = $derived(activities.length < total);
</script>

<div class="chips">
  <button class:active={category === ''} class="chip" onclick={() => (category = '')}>{$t('timeline.filter-all')}</button>
  {#each CATEGORIES as c}
    <button class:active={category === c} class="chip" onclick={() => (category = c)}>{$t(`cat.${c}`)}</button>
  {/each}
</div>

{#if activities.length === 0}
  <p class="muted">{$t('timeline.empty')}</p>
{:else}
  {#each groups as [year, items] (year)}
    <p class="year">{year}</p>
    <div class="list">
      {#each items as a (a.id)}
        <button
          class="card entry"
          class:pending={a.pending}
          disabled={a.pending}
          onclick={() => go(`/objects/${objectId}/activities/${a.id}`)}
        >
          <div class="row head">
            <b>{a.title}</b>
            <span class="chip">{$t(`cat.${a.category}`)}</span>
            {#if a.pending}<span class="chip pending-chip">{$t('timeline.pending')}</span>{/if}
          </div>
          <div class="muted tnum">
            {fmtDate(a.date, $locale)}
            {#if a.counter_value !== null} · {counter(a.counter_value, unit, $locale)}{/if}
            {#if a.cost_cents !== null} · {money(a.cost_cents, $currency, $locale)}{/if}
          </div>
          {#if a.notes}<p class="notes">{a.notes}</p>{/if}
          {#if a.attachments.length > 0}
            <div class="thumb-strip">
              {#each a.attachments.slice(0, 6) as att (att.id)}
                {#if att.kind === 'photo'}<img src={fileUrl(att.file_id, true)} alt="" loading="lazy" />{:else}<span class="doc-chip">📄</span>{/if}
              {/each}
            </div>
          {/if}
        </button>
      {/each}
    </div>
  {/each}
  {#if hasMore}
    <button class="more" onclick={() => onmore?.()} disabled={loadingMore}>
      {loadingMore ? $t('nav.loading') : $t('timeline.more', { n: total - activities.length })}
    </button>
  {/if}
{/if}

<style>
  .entry { display: flex; flex-direction: column; gap: 4px; text-align: left; width: 100%; }
  .entry.pending { opacity: .55; cursor: default; }
  .pending-chip { flex: none; }
  .head { justify-content: space-between; }
  .head b { flex: 1; }
  .head .chip { flex: none; }
  .notes { font-size: .9rem; white-space: pre-wrap; }
  .more { width: 100%; margin-top: 12px; }
  .doc-chip { display: grid; place-items: center; width: 64px; height: 64px; background: var(--surface-2); border-radius: 6px; }
</style>
