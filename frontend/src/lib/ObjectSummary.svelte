<script lang="ts">
  import { fileUrl } from './api';
  import TagChips from './TagChips.svelte';
  import { counter, money, quantity } from './format';
  import { fuelUnitLabel } from './energy';
  import { sinceLabel } from './insights';
  import { lateness } from './lateness';
  import { figureKeys, type FigureKey } from './object-detail';
  import { formatWeight } from './weight';
  import { customTypes, typeLabel, typesLoaded } from './type-registry';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { Insights, MemObject, Reminder } from './types';

  /** The top of the object page: what it is and where it stands. One piece of markup for both
   *  layouts -- above the tabs on a phone, the head of the left pane from 1024 px; CSS moves it
   *  (see routes/ObjectDetail.svelte). `insights` is `null` until loaded; only the consumption
   *  figure waits for it. `due` is the object's due reminders; tapping one opens the Reminders
   *  tab, where the actions are. */
  let { object, insights, due, onreminders }: {
    object: MemObject; insights: Insights | null; due: Reminder[]; onreminders: () => void;
  } = $props();

  const LABEL: Record<FigureKey, string> = {
    cost: 'object.total', weight: 'weight.latest', counter: 'object.current', usage: 'object.per-month',
    consumption: 'insights.consumption', activities: 'object.activities',
  };
  const keys = $derived(figureKeys(object, insights?.fuel?.per_100_milli != null));

  function value(key: FigureKey): string {
    const s = object.stats;
    switch (key) {
      case 'cost': return money(s.total_cost_cents, $currency, $locale);
      case 'weight': return formatWeight(s.latest_weight_grams ?? 0, object.weight_unit ?? 'kg', $locale);
      case 'counter': return counter(s.current_counter, object.counter_unit, $locale) || '—';
      // A month is the unit people think in; rounded to whole units, since the rate is an average.
      case 'usage': return counter(Math.round((s.counter_per_day_milli ?? 0) * 30.44 / 1000), object.counter_unit, $locale);
      case 'consumption': {
        const f = insights?.fuel;
        return f?.per_100_milli != null ? `${quantity(f.per_100_milli, fuelUnitLabel(f.unit), $locale)}/100 ${object.counter_unit}` : '—';
      }
      case 'activities': return String(s.activity_count);
    }
  }

  /** Stretched link, as on the dashboard: the title's ::after covers the card, so all of it is the
   *  target, and the ring is drawn on that ::after. */
  const stretched = "after:absolute after:inset-0 after:rounded-lg after:content-[''] focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:outline-solid focus-visible:after:outline-offset-2 focus-visible:after:outline-ring";
</script>

{#if object.cover_file_id}
  <!-- A fixed 16:10 box, cropped to fit: the old free-height hero cut faces and number plates at
       whatever height the photo happened to have. The thumbnail first (usually cached from the
       dashboard card), the original over it once loaded; keyed so a new cover starts from its own
       thumbnail. -->
  {#key object.cover_file_id}
    <div data-testid="object-cover" class="relative aspect-[16/10] w-full overflow-hidden rounded-lg bg-muted">
      <img class="absolute inset-0 block size-full object-cover" src={fileUrl(object.cover_file_id, true)} alt="" decoding="async" />
      <img class="absolute inset-0 block size-full object-cover opacity-0 transition-opacity duration-150" src={fileUrl(object.cover_file_id)} alt="" decoding="async"
           onload={(e) => e.currentTarget.classList.replace('opacity-0', 'opacity-100')} />
    </div>
  {/key}
{/if}

<div class="flex flex-col gap-1.5">
  <p class="m-0 text-sm text-muted-foreground">
    <span data-testid="object-type">{typeLabel(object.type, $customTypes, $t, $typesLoaded)}</span>{#if object.purchase_date}{' · '}{$t('object.since-short', { date: sinceLabel(object.purchase_date, $locale) })}{/if}
  </p>
  {#if (object.tags ?? []).length > 0}<div class="w-fit"><TagChips tags={object.tags} /></div>{/if}
</div>

{#if keys.length > 0}
  <!-- A strip that scrolls sideways on a phone (the fade says there is more), a 2×2 grid in the
       pane. Focusable, so a keyboard can scroll it; the fade is dropped while it has focus so it
       does not dim the ring. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div role="group" aria-label={$t('object.figures')} tabindex="0"
       class="-m-1 overflow-x-auto p-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden max-wide:[mask-image:linear-gradient(to_right,#000_calc(100%_-_24px),transparent)] max-wide:focus-visible:[mask-image:none] focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring wide:overflow-visible">
    <dl data-testid="figures" class="m-0 flex gap-2 wide:grid wide:grid-cols-2">
      {#each keys as key (key)}
        <div data-testid={`figure-${key}`} class="flex min-w-32 shrink-0 flex-col rounded-lg border border-border bg-card p-3 shadow-xs wide:min-w-0">
          <!-- Label first for a screen reader, number first for the eye. -->
          <dt class="order-2 text-xs text-muted-foreground">{$t(LABEL[key])}</dt>
          <dd class="order-1 m-0 truncate text-lg font-semibold text-foreground tabular-nums">{value(key)}</dd>
        </div>
      {/each}
    </dl>
  </div>
{/if}

{#if due.length > 0}
  <div class="flex flex-col gap-2">
    {#each due as r (r.id)}
      <div data-testid="summary-due-reminder" class="relative flex items-center gap-3 rounded-lg border border-destructive/30 bg-destructive/10 p-3">
        <span class="size-2 shrink-0 rounded-full bg-destructive" aria-hidden="true"></span>
        <div class="min-w-0 flex-1">
          <a href={`/objects/${object.id}?tab=reminders`} class={`block break-words font-semibold text-foreground no-underline ${stretched}`}
             onclick={(e) => { e.preventDefault(); onreminders(); }}>{r.title}</a>
          {#if lateness(r, $t)}<p class="m-0 text-sm text-muted-foreground">{lateness(r, $t)}</p>{/if}
        </div>
      </div>
    {/each}
  </div>
{/if}
