<script lang="ts">
  import { counter, fmtDate, sinceCounter } from './format';
  import { activityTitle } from './activity-form';
  import { dateFormat } from '../stores/date-format';
  import { locale, t } from '../i18n';
  import type { LastDone, MemObject } from './types';

  /** `items` is loaded by the parent (like `loadChildren`), not by this component -- it is
   *  fetched once, when the details are shown (Info tab, or the desktop pane), not on every render of this list. Hidden entirely
   *  when there is nothing to show: an empty "Last done" heading over blank space would read as
   *  a bug, not as "nothing repeats yet". */
  let { items, object, onselect }: { items: LastDone[]; object: MemObject; onselect: (title: string) => void } = $props();
</script>

{#if items.length > 0}
  <section>
    <h2 class="m-0 mb-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase">{$t('lastdone.title')}</h2>
    <div class="flex flex-col gap-2">
      {#each items as row (row.last_activity_id)}
        {@const since = sinceCounter(object.stats.current_counter, row.last_counter)}
        <!-- `onselect(row.title)` passes the real title: the timeline's title filter matches on it. -->
        <button data-slot="last-done-row" data-testid="last-done-row" onclick={() => onselect(row.title)}
                class="flex min-h-11 w-full cursor-pointer flex-col gap-0.5 rounded-lg border border-border bg-card p-3 text-left shadow-xs transition-colors hover:border-input focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">
          <span class="font-semibold text-foreground">{activityTitle(row.title, undefined, $t)}</span>
          <span class="text-sm text-muted-foreground tabular-nums">
            {fmtDate(row.last_date, $dateFormat)}
            {#if row.last_counter !== null} · {counter(row.last_counter, object.counter_unit, $locale)}{/if}
            {#if since !== null} · {$t('lastdone.since', { amount: counter(since, object.counter_unit, $locale) })}{/if}
          </span>
        </button>
      {/each}
    </div>
  </section>
{/if}
