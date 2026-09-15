<script lang="ts">
  import { counter, fmtDate, sinceCounter } from './format';
  import { activityTitle } from './activity-form';
  import { dateFormat } from '../stores/date-format';
  import { locale, t } from '../i18n';
  import type { LastDone, MemObject } from './types';

  /** `items` is loaded by the parent (like `loadChildren`), not by this component -- it is
   *  fetched once, when the Info tab opens, not on every render of this list. Hidden entirely
   *  when there is nothing to show: an empty "Last done" heading over blank space would read as
   *  a bug, not as "nothing repeats yet". */
  let { items, object, onselect }: { items: LastDone[]; object: MemObject; onselect: (title: string) => void } = $props();
</script>

{#if items.length > 0}
  <h3>{$t('lastdone.title')}</h3>
  <div class="list">
    {#each items as row (row.last_activity_id)}
      {@const since = sinceCounter(object.stats.current_counter, row.last_counter)}
      <!-- `onselect(row.title)` keeps passing the real (possibly empty) title -- the timeline's
           title filter matches on that exact value -- only the visible text below falls back to
           "Trip" for an untitled one, same as any other place a trip's title is shown. -->
      <button class="card entry" onclick={() => onselect(row.title)}>
        <b>{activityTitle(row.title, $t)}</b>
        <span class="muted tnum">
          {fmtDate(row.last_date, $dateFormat)}
          {#if row.last_counter !== null} · {counter(row.last_counter, object.counter_unit, $locale)}{/if}
          {#if since !== null} · {$t('lastdone.since', { amount: counter(since, object.counter_unit, $locale) })}{/if}
        </span>
      </button>
    {/each}
  </div>
{/if}

<style>
  .entry { display: flex; flex-direction: column; gap: var(--space-1); text-align: left; width: 100%; }
</style>
