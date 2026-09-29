<script lang="ts">
  import { go } from './router';
  import { lateness as latenessOf } from './lateness';
  import { t } from '../i18n';
  import { fmtDate } from './format';
  import { dateFormat } from '../stores/date-format';
  import type { Reminder } from './types';

  let { due, soon, onsnooze }: { due: Reminder[]; soon: Reminder[]; onsnooze: (r: Reminder) => void } = $props();

  /** See ./lateness.ts. */
  const lateness = (r: Reminder) => latenessOf(r, $t);

  const open = (r: Reminder) => go(`/objects/${r.object_id}?tab=reminders`);
  /** Stretched link: the title's ::after covers the whole card/row, so all of it opens the object;
   *  the ring is drawn on that ::after. Buttons sit above it (`relative z-10`). */
  const stretched = "after:absolute after:inset-0 after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-solid focus-visible:after:outline-offset-2 focus-visible:after:outline-ring";
  const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring';
</script>

<!-- One card per due reminder: the thing to act on, with the action on it. Tinted, not filled --
     a solid red block took the whole first screen and made everything under it look secondary.
     No object tags: they describe the object, and on a reminder they read as the reminder's. -->
{#if due.length > 0}
  <section class="mb-4 flex flex-col gap-2" aria-label={due.length === 1 ? $t('dash.due-one') : $t('dash.due', { n: due.length })}>
    {#each due as r (r.id)}
      <div data-testid="due-reminder" class="relative flex items-center gap-3 rounded-lg border border-destructive/30 bg-destructive/10 p-3">
        <span class="size-2 shrink-0 rounded-full bg-destructive" aria-hidden="true"></span>
        <div class="min-w-0 flex-1">
          <a href={`/objects/${r.object_id}?tab=reminders`} class={`block break-words font-semibold text-foreground no-underline after:rounded-lg ${stretched}`}
             onclick={(e) => { e.preventDefault(); open(r); }}>{r.title}</a>
          <p class="m-0 text-sm text-muted-foreground">{r.object_name}{#if lateness(r)}{' · '}{lateness(r)}{/if}</p>
        </div>
        {#if r.kind === 'reading'}
          <!-- The whole job is one number, so it is one tap from here. -->
          <button data-slot="dash-action" class={`relative z-10 min-h-11 shrink-0 cursor-pointer rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground ${focus}`}
                  onclick={() => go(`/objects/${r.object_id}/reading`)}>{$t('reminder.record')}</button>
        {/if}
        <button data-slot="dash-action" class={`relative z-10 min-h-11 shrink-0 cursor-pointer rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground ${focus}`}
                onclick={() => onsnooze(r)}>{$t('reminder.snooze')}</button>
      </div>
    {/each}
  </section>
{/if}

{#if soon.length > 0}
  <section class="mb-4 rounded-lg border border-border bg-card p-3">
    <h2 class="m-0 mb-1 text-xs font-semibold tracking-wide text-muted-foreground uppercase">{$t('dash.upcoming')}</h2>
    {#each soon as r (r.id)}
      <div data-testid="upcoming-reminder" class="relative flex min-h-11 items-center justify-between gap-3 border-t border-border py-2 first-of-type:border-t-0">
        <a href={`/objects/${r.object_id}?tab=reminders`} class={`min-w-0 truncate text-foreground no-underline after:rounded-md ${stretched}`}
           onclick={(e) => { e.preventDefault(); open(r); }}><span class="font-medium">{r.title}</span> <span class="text-muted-foreground">· {r.object_name}</span></a>
        <span class="shrink-0 text-sm text-muted-foreground tabular-nums">
          {#if r.days_until !== null}{r.days_until === 1 ? $t('dash.in-day') : $t('dash.in-days', { n: r.days_until })}{/if}
          {#if r.counter_until !== null && r.counter_unit}{r.days_until !== null ? ' · ' : ''}{$t('dash.in-counter', { n: r.counter_until, unit: r.counter_unit })}{/if}
          {#if r.estimated_due_date} · {$t('dash.estimated', { date: fmtDate(r.estimated_due_date, $dateFormat) })}{/if}
        </span>
      </div>
    {/each}
  </section>
{/if}
