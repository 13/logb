<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import { api } from '../lib/api';
  import { go, back } from '../lib/router';
  import { t } from '../i18n';
  import { centsToInput, parseMoney } from '../lib/format';
  import { emptyInput, toInput, validate } from '../lib/object-form';
  import { OBJECT_TYPES, type MemObject, type ObjectInput } from '../lib/types';

  let { id }: { id?: string } = $props();
  const editing = $derived(id !== undefined);
  let input = $state<ObjectInput>(emptyInput());
  let priceText = $state('');
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    if (!id) return;
    const o = await api<MemObject>('GET', `/objects/${id}`);
    input = toInput(o);
    priceText = centsToInput(o.purchase_price_cents);
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    input.purchase_price_cents = parseMoney(priceText);
    const bad = validate(input);
    if (bad) { error = $t(bad); return; }
    busy = true; error = '';
    try {
      if (!input.purchase_date) input.purchase_date = null;
      const saved = editing
        ? await api<MemObject>('PATCH', `/objects/${id}`, input)
        : await api<MemObject>('POST', '/objects', input);
      go(`/objects/${saved.id}`, true);
    } catch (err) { error = (err as Error).message; } finally { busy = false; }
  }

  async function remove() {
    if (!confirm($t('nav.confirm-delete'))) return;
    await api('DELETE', `/objects/${id}`);
    go('/', true);
  }
</script>

<main>
  <TopBar title={editing ? $t('object.edit') : $t('object.new')} backTo={editing ? `/objects/${id}` : '/'} />
  <form onsubmit={submit}>
    <div class="field"><label for="n">{$t('object.name')}</label><input id="n" bind:value={input.name} required /></div>
    <div class="field">
      <label for="c">{$t('object.type')}</label>
      <select id="c" bind:value={input.type}>
        {#each OBJECT_TYPES as ty}<option value={ty}>{$t(`type.${ty}`)}</option>{/each}
      </select>
    </div>
    <div class="field">
      <label for="u">{$t('object.counter')}</label>
      <select id="u" bind:value={input.counter_unit}>
        <option value={null}>{$t('object.counter-none')}</option>
        <option value="km">{$t('object.counter-km')}</option>
        <option value="mi">{$t('object.counter-mi')}</option>
        <option value="h">{$t('object.counter-h')}</option>
      </select>
    </div>
    <div class="field">
      <label for="fu">{$t('object.fuel-unit')}</label>
      <select id="fu" bind:value={input.fuel_unit}>
        <option value={null}>{$t('object.counter-none')}</option>
        <option value="l">l</option>
        <option value="gal">gal</option>
        <option value="kwh">kwh</option>
      </select>
    </div>
    <div class="field"><label for="d">{$t('object.description')}</label><textarea id="d" bind:value={input.description}></textarea></div>
    <div class="row">
      <div class="field"><label for="pd">{$t('object.purchase-date')}</label><input id="pd" type="date" bind:value={input.purchase_date} /></div>
      <div class="field"><label for="pp">{$t('object.purchase-price')}</label><input id="pp" type="text" inputmode="decimal" bind:value={priceText} /></div>
    </div>
    {#if editing}
      <label class="row toggle"><input type="checkbox" bind:checked={input.archived} /> {$t('object.archive')}</label>
      <p class="hint">{$t('object.archived-hint')}</p>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    <div class="row actions">
      <button type="button" class="ghost" onclick={() => back(editing ? `/objects/${id}` : '/')}>{$t('nav.cancel')}</button>
      <button class="primary" disabled={busy}>{$t('nav.save')}</button>
    </div>
  </form>
  {#if editing}
    <h2>{$t('object.delete')}</h2>
    <p class="hint">{$t('object.delete-hint')}</p>
    <button class="danger" onclick={remove}>{$t('object.delete')}</button>
  {/if}
</main>

<style>
  .toggle input { flex: none; width: 20px; height: 20px; }
  .actions { margin-top: var(--space-2); }
</style>
