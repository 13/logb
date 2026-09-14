<script lang="ts">
  import { go } from './router';
  import { fileUrl } from './api';
  import { counter, money, lastActivityLabel, todayIso } from './format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { MemObject } from './types';
  import { typeIcon } from './object-types';
  import Icon from './Icon.svelte';
  import TagChips from './TagChips.svelte';
  /** `ontag` makes the chips buttons that filter by their tag; without it they are plain labels. */
  let { object, parentName = null, ontag }: { object: MemObject; parentName?: string | null; ontag?: (tag: string) => void } = $props();
</script>

<!-- The quick-log action belongs to this object, so it sits inside the object's card rather than
     in a box of its own beside it -- a row with a gap on either side of a second full-height box
     reads as two cards, not as one thing you can act on. A button cannot be nested inside a
     button, and the card is a button: the whole row navigates, and 26 e2e tests find objects by
     that button's accessible name. So the card stays the button and the quick-log action is a
     sibling positioned within its bounds. Both are in the tab order, in reading order, and the
     card's padding-right keeps its text from running under the action. The tag chips can be
     buttons too, so they sit below the card rather than in it; `.card-box` is what the action is
     centred on, so the chips' height does not pull it down off the card. -->
<div class="card-row">
  <div class="card-box">
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
      {#if parentName}<div class="muted small">{$t('search.in-parent', { name: parentName })}</div>{/if}
      <div class="muted tnum type-row">
        <Icon name={typeIcon(object.type)} size={16} />
        {$t(`type.${object.type}`)}
        {#if object.stats.current_counter !== null} · {counter(object.stats.current_counter, object.counter_unit, $locale)}{/if}
        <!-- A month is the unit people think in; rounded like the Info tab, since it is an average. -->
        {#if object.stats.counter_per_day_milli !== null && object.counter_unit} · {$t('insights.per-month', { amount: counter(Math.round(object.stats.counter_per_day_milli * 30.44 / 1000), object.counter_unit, $locale) })}{/if}
        {#if object.stats.total_cost_cents > 0} · {money(object.stats.total_cost_cents, $currency, $locale)}{/if}
        {#if object.stats.last_activity_date} · {lastActivityLabel(object.stats.last_activity_date, todayIso(), $locale)}{/if}
      </div>
    </div>
  </button>
  <button class="quicklog" aria-label={$t('dash.log')}
          onclick={() => go(`/objects/${object.id}/activities/new`)}><Icon name="plus" /></button>
  </div>
  {#if (object.tags ?? []).length > 0}
    <div class="card-tags"><TagChips tags={object.tags} onselect={ontag} /></div>
  {/if}
</div>

<style>
  .card-box { position: relative; }
  /* Inset by the card's own padding, so the chips start where the card's content does. */
  .card-tags { margin-top: var(--space-1); padding-left: var(--space-3); }
  .list-card {
    display: flex; gap: var(--space-3); align-items: center; text-align: left; width: 100%;
    background: var(--surface); border: 1px solid var(--border);
    /* Room for the action: its own width, plus the inset on each side of it. 44px is the tap
       target -- an accessibility floor, not a spacing step -- so this gutter is not a scale
       value and cannot be one. It is still spacing, so it is named in the spacing namespace
       and everything about it that the scale *can* say (the insets) comes from the scale. */
    --space-quicklog-gutter: calc(44px + var(--space-2) * 2);
    padding-right: var(--space-quicklog-gutter);
  }
  /* A filled square on the card's surface, not an outlined box beside it: it reads as a control
     within the object rather than as a second object. 44px is the tap target the rest of the
     app uses. */
  .quicklog {
    position: absolute; right: var(--space-2); top: 50%; transform: translateY(-50%);
    width: 44px; min-height: 44px; padding: 0;
    display: grid; place-items: center;
    background: var(--surface-2); border-radius: var(--radius-sm);
  }
  .cover { width: 64px; height: 64px; flex: none; }
  .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: var(--space-1); }
  .row { display: flex; gap: var(--space-2); align-items: center; flex-wrap: wrap; }
  .row > b { flex: none; }
  /* Flex would put the icon and each piece of text (split into separate runs by Svelte's block
     anchors) into their own wrappable items, so the icon can land alone on its own line. Block
     flow keeps the icon inline with the text that follows it, wrapping naturally as one run. */
  .type-row { display: block; }
  .type-row :global(svg) { vertical-align: middle; margin-right: var(--space-2); }
</style>
