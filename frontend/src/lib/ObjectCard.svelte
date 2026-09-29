<script lang="ts">
  import { formatWeight } from './weight';
  import { go } from './router';
  import { fileUrl } from './api';
  import { counter, money, lastActivityLabel, todayIso } from './format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { MemObject } from './types';
  import { customTypes, typeIcon, typeLabel, typesLoaded } from './type-registry';
  import Icon from './Icon.svelte';
  import TagChips from './TagChips.svelte';
  /** `ontag` makes the chips buttons that filter by their tag; without it they are plain labels. */
  /** `activeTag`: the list's tag filter, so the chip it matches shows as pressed. */
  let { object, parentName = null, ontag, activeTag = null }: { object: MemObject; parentName?: string | null; ontag?: (tag: string) => void; activeTag?: string | null } = $props();
</script>

<!-- The card is one box holding everything about the object. The whole box opens the object:
     the main button's ::after covers the card (`after:absolute after:inset-0`), so the name and
     facts are one big target and 26 e2e tests still find the object by that button's name. The
     tag chips are buttons of their own and cannot sit inside another button, so they are
     siblings raised above the stretched target (`relative z-10`). -->
<article data-testid="object-card" class="relative flex gap-3 rounded-lg border border-border bg-card p-3 shadow-xs transition-colors hover:border-input">
  {#if object.cover_file_id}
    <img class="size-12 shrink-0 rounded-md object-cover" src={fileUrl(object.cover_file_id, true)} alt="" loading="lazy" decoding="async" />
  {:else}
    <span class="grid size-12 shrink-0 place-items-center rounded-md bg-primary/10 text-brand-ink" aria-hidden="true">
      <Icon name={typeIcon(object.type, $customTypes)} size={22} />
    </span>
  {/if}
  <div class="flex min-w-0 flex-1 flex-col gap-1">
    <div class="flex items-start gap-2">
      <button data-slot="card-open"
              class="min-w-0 flex-1 cursor-pointer line-clamp-2 break-words text-left text-base font-semibold text-foreground after:absolute after:inset-0 after:rounded-lg after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-solid focus-visible:after:outline-offset-2 focus-visible:after:outline-ring"
              onclick={() => go(`/objects/${object.id}`)}>{object.name}</button>
      {#if object.stats.due_reminder_count > 0}
        <span data-testid="due-badge" class="shrink-0 rounded-full bg-destructive/10 px-2 py-0.5 text-xs font-semibold text-destructive">
          {object.stats.due_reminder_count === 1 ? $t('dash.due-one') : $t('dash.due', { n: object.stats.due_reminder_count })}
        </span>
      {/if}
      {#if object.archived_at}<span class="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('dash.archived')}</span>{/if}
      {#if object.pending}<span class="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{$t('timeline.pending')}</span>{/if}
    </div>
    {#if parentName}<p class="m-0 truncate text-xs text-muted-foreground">{$t('search.in-parent', { name: parentName })}</p>{/if}
    <p class="m-0 text-sm text-muted-foreground tabular-nums">
      {typeLabel(object.type, $customTypes, $t, $typesLoaded)}
      {#if object.type === 'body' && object.stats.latest_weight_grams != null} · {formatWeight(object.stats.latest_weight_grams, object.weight_unit ?? 'kg', $locale)}{/if}
      {#if object.stats.current_counter !== null} · {counter(object.stats.current_counter, object.counter_unit, $locale)}{/if}
      <!-- A month is the unit people think in; rounded like the Info tab, since it is an average. -->
      {#if object.stats.counter_per_day_milli !== null && object.counter_unit} · {$t('insights.per-month', { amount: counter(Math.round(object.stats.counter_per_day_milli * 30.44 / 1000), object.counter_unit, $locale) })}{/if}
      {#if object.stats.total_cost_cents > 0} · {money(object.stats.total_cost_cents, $currency, $locale)}{/if}
      {#if object.stats.last_activity_date} · {$t('dash.last-entry', { when: lastActivityLabel(object.stats.last_activity_date, todayIso(), $locale) })}{/if}
    </p>
    {#if (object.tags ?? []).length > 0}
      <div class="relative z-10 mt-1 w-fit"><TagChips tags={object.tags} onselect={ontag} active={activeTag} /></div>
    {/if}
  </div>
</article>
