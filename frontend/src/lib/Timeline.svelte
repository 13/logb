<script lang="ts">
  import { fileUrl } from './api';
  import { go } from './router';
  import { counter, fmtDate, money } from './format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import { groupByYear } from './activity-form';
  import { categoriesFor } from './object-types';
  import { CATEGORIES, type Activity, type Category, type CounterUnit, type ObjectType } from './types';
  import Icon from './Icon.svelte';

  let { objectId, type, activities, total, loadingMore = false, onmore, onlog, unit, category = $bindable('') }:
    {
      objectId: number; type: ObjectType; activities: Activity[]; total: number; loadingMore?: boolean;
      onmore?: () => void; onlog?: () => void; unit: CounterUnit; category?: Category | '';
    } = $props();
  const groups = $derived(groupByYear(activities));
  const hasMore = $derived(activities.length < total);
  // The type's vocabulary, plus any category the loaded entries actually use. The second half
  // matters after a re-type: without it, an entry logged as `fuel` on an object that is now a
  // `body` has no chip and cannot be filtered to at all. `activities` is only the page(s)
  // loaded so far, not necessarily every entry the object has -- acceptable here since this is
  // presentation, not the source of truth for what exists.
  const present = $derived(new Set(activities.map((a) => a.category)));
  const chipCategories = $derived(
    [...categoriesFor(type), ...CATEGORIES.filter((c) => present.has(c))]
      .filter((c, i, all) => all.indexOf(c) === i),
  );
</script>

<div class="chips">
  <button class:active={category === ''} class="chip" onclick={() => (category = '')}>{$t('timeline.filter-all')}</button>
  {#each chipCategories as c}
    <button class:active={category === c} class="chip" onclick={() => (category = c)}>{$t(`cat.${c}`)}</button>
  {/each}
</div>

{#if activities.length === 0}
  <!-- An object with no history and an object whose filter matched nothing are not the same
       screen: the first is an invitation, the second is a fact about the chip above it. -->
  <div class="empty">
    {#if category === ''}
      <span class="empty-icon"><Icon name="edit" size={40} /></span>
      <p>{$t('timeline.empty')}</p>
      {#if onlog}<button class="primary" onclick={() => onlog()}>+ {$t('timeline.log')}</button>{/if}
    {:else}
      <p>{$t('timeline.none-in-filter')}</p>
    {/if}
  </div>
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
                {#if att.kind === 'photo'}<img src={fileUrl(att.file_id, true)} alt="" loading="lazy" />{:else}<span class="doc-chip"><Icon name="document" size={28} /></span>{/if}
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
  .entry { display: flex; flex-direction: column; gap: var(--space-1); text-align: left; width: 100%; }
  .entry.pending { opacity: .55; cursor: default; }
  .pending-chip { flex: none; }
  .head { justify-content: space-between; }
  .head b { flex: 1; }
  .head .chip { flex: none; }
  .notes { font-size: var(--text-sm); white-space: pre-wrap; }
  .more { width: 100%; margin-top: var(--space-3); }
  .doc-chip { display: grid; place-items: center; width: 64px; height: 64px; background: var(--surface-2); border-radius: var(--radius-sm); }
</style>
