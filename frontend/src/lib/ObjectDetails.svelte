<script lang="ts">
  import { Button } from '$lib/components/ui/button/index.js';
  import { go } from './router';
  import Chart from './Chart.svelte';
  import Insights from './Insights.svelte';
  import LastDone from './LastDone.svelte';
  import TripTotals from './TripTotals.svelte';
  import EnergyFigures from './EnergyFigures.svelte';
  import ObjectCard from './ObjectCard.svelte';
  import ResourceCsvImport from './ResourceCsvImport.svelte';
  import { money } from './format';
  import { monthLabel, monthTick } from './insights';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { EnergyOut, Insights as InsightsData, LastDone as LastDoneT, MemObject, TripSummary } from './types';

  /** Everything about the object that is not its summary or its history: spend per month, its
   *  description, last done, trips, energy, contents, the cost breakdown, and the two actions
   *  that close it. Rendered in exactly one place -- the left pane from 1024 px, the Info tab
   *  below that; ObjectDetail picks, and loads the data it takes. */
  let { object, insights, insightsError, lastDone, tripSummary, energy, inside, hasContents, contents, offersTrip, offersEnergy, oncontents, onlastdone, onimported }: {
    object: MemObject; insights: InsightsData | null; insightsError: string; lastDone: LastDoneT[];
    tripSummary: TripSummary | null; energy: EnergyOut | null;
    /** The object's live children, listed under Contents. */
    inside: MemObject[];
    hasContents: boolean; contents: boolean; offersTrip: boolean; offersEnergy: boolean;
    oncontents: (on: boolean) => void; onlastdone: (title: string) => void; onimported: () => void;
  } = $props();

  const heading = 'm-0 mb-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase';
  const spend = $derived(insights && insights.by_month.some((b) => b.cost_cents > 0) ? insights.by_month : null);
</script>

<div class="flex flex-col gap-6">
  {#if spend}
    <section data-testid="insights-spend">
      <h2 class={heading}>{$t('insights.spend-by-month')}</h2>
      <Chart label={$t('insights.spend-by-month')}
             items={spend.map((b) => ({ key: b.bucket, label: monthLabel(b.bucket, $locale), tick: monthTick(b.bucket, $locale), value: b.cost_cents, display: money(b.cost_cents, $currency, $locale) }))} />
    </section>
  {/if}

  {#if object.description || object.purchase_price_cents !== null}
    <section class="flex flex-col gap-1">
      {#if object.description}<p class="m-0 whitespace-pre-wrap text-sm text-foreground">{object.description}</p>{/if}
      {#if object.purchase_price_cents !== null}
        <p class="m-0 text-sm text-muted-foreground">{$t('object.purchase-price')}: {money(object.purchase_price_cents, $currency, $locale)}</p>
      {/if}
    </section>
  {/if}

  <LastDone items={lastDone} {object} onselect={onlastdone} />
  {#if offersTrip}
    <TripTotals summary={tripSummary} unit={object.counter_unit as 'km' | 'mi'} energyRate={energy?.cost_per_counter_milli ?? null} />
  {/if}
  {#if offersEnergy}<EnergyFigures {energy} unit={object.counter_unit} />{/if}

  <section>
    <h2 class={heading}>{$t('object.contents')}</h2>
    {#if inside.length === 0}
      <p class="m-0 text-sm text-muted-foreground">{$t('object.contents-empty')}</p>
    {:else}
      <div class="grid grid-cols-1 gap-3">
        {#each inside as c (c.id)}<ObjectCard object={c} />{/each}
      </div>
    {/if}
  </section>

  <section>
    <h2 class={heading}>{$t('insights.title')}</h2>
    <Insights data={insights} error={insightsError} unit={object.counter_unit} {hasContents} {contents} {oncontents} />
  </section>

  {#if object.resource_kind}<ResourceCsvImport objectId={object.id} mode={object.measurement_mode} {onimported} />{/if}

  <div class="flex flex-col gap-2">
    <Button variant="outline" class="min-h-11 w-full" onclick={() => go(`/objects/new?parent_id=${object.id}`)}>+ {$t('object.contents-add')}</Button>
    <Button variant="outline" class="min-h-11 w-full" href={`/api/export?object_id=${object.id}`}>{$t('object.export')}</Button>
  </div>
</div>
