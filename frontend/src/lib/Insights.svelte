<script lang="ts">
  import { api } from './api';
  import { money, perCounter, quantity } from './format';
  import { currency } from '../stores/session';
  import { locale, t } from '../i18n';
  import type { CounterUnit, Insights } from './types';

  let { objectId, unit }: { objectId: number; unit: CounterUnit } = $props();

  let data = $state<Insights | null>(null);
  let error = $state('');

  $effect(() => {
    objectId;
    api<Insights>('GET', `/objects/${objectId}/insights`)
      .then((d) => (data = d))
      .catch((e) => (error = (e as Error).message));
  });

  /** Bar width as a percentage of the largest bucket, so the widest bar always fills the row. */
  function pct(value: number, all: { cost_cents: number }[]): number {
    const max = Math.max(...all.map((b) => b.cost_cents), 1);
    return Math.round((value / max) * 100);
  }
</script>

{#if error}<p class="error">{error}</p>{/if}
{#if data}
  {#if data.by_year.length === 0}
    <p class="muted">{$t('insights.none')}</p>
  {:else}
    <h3>{$t('insights.by-year')}</h3>
    {#each data.by_year as b (b.bucket)}
      <div class="bar-row">
        <span class="label">{b.bucket}</span>
        <span class="track"><span class="fill" style={`width:${pct(b.cost_cents, data.by_year)}%`}></span></span>
        <span class="value">{money(b.cost_cents, $currency, $locale)}</span>
      </div>
    {/each}

    <h3>{$t('insights.by-category')}</h3>
    {#each data.by_category as b (b.bucket)}
      <div class="bar-row">
        <span class="label">{$t(`cat.${b.bucket}`)}</span>
        <span class="track"><span class="fill" style={`width:${pct(b.cost_cents, data.by_category)}%`}></span></span>
        <span class="value">{money(b.cost_cents, $currency, $locale)}</span>
      </div>
    {/each}

    {#if data.cost_per_counter_milli !== null && unit}
      <p class="muted">{$t('insights.per-counter', { unit })}: <b>{perCounter(data.cost_per_counter_milli, $currency, $locale)}</b></p>
    {/if}
    {#if data.fuel}
      <p class="muted">{$t('insights.fuel-total')}: <b>{quantity(data.fuel.quantity_milli, data.fuel.unit, $locale)}</b></p>
      {#if data.fuel.per_100_milli !== null && unit}
        <p class="muted">{$t('insights.consumption')}: <b>{quantity(data.fuel.per_100_milli, data.fuel.unit, $locale)}/100 {unit}</b></p>
      {/if}
    {/if}
  {/if}
{/if}

<style>
  h3 { margin: 16px 0 8px; font-size: 1rem; }
  .bar-row { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; }
  .label { flex: none; width: 90px; font-size: .9rem; }
  .track { flex: 1; height: 10px; background: var(--surface-2); border-radius: 5px; overflow: hidden; }
  .fill { display: block; height: 100%; background: var(--accent); }
  .value { flex: none; font-size: .9rem; }
</style>
