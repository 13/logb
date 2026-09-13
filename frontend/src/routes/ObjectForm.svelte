<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import { api } from '../lib/api';
  import { go, back } from '../lib/router';
  import { t } from '../i18n';
  import { centsToInput, parseMoney } from '../lib/format';
  import { emptyInput, toInput, validate } from '../lib/object-form';
  import { excludingDescendants } from '../lib/object-tree';
  import { fieldError } from '../lib/form-error';
  import { readingReminder, reminderBody } from '../lib/reminder-form';
  import { addMonthsIso } from '../lib/reading';
  import { todayIso } from '../lib/format';
  import { OBJECT_TYPES, type MemObject, type ObjectInput } from '../lib/types';

  let { id }: { id?: string } = $props();
  const editing = $derived(id !== undefined);
  const presetParentId = new URLSearchParams(location.search).get('parent_id');
  let input = $state<ObjectInput>(
    untrack(() => (id === undefined ? { ...emptyInput(), parent_id: presetParentId ? Number(presetParentId) : null } : emptyInput())),
  );
  let priceText = $state('');
  /** Opt-in, never automatic: a reminder nobody asked for is the kind that gets muted. */
  let remindReading = $state(false);
  let error = $state('');
  let busy = $state(false);
  /** The objects offered as this one's parent: everything the user owns, minus this object and
   *  its descendants (which the server would refuse as a cycle), minus anything archived. */
  let parentChoices = $state<MemObject[]>([]);
  /** Name by id across the *whole* tree, archived rows included, so an option can say which
   *  object it sits inside even when that container is itself archived. */
  let nameById = $state(new Map<number, string>());

  /** An option's text: the object's name plus the name of the object it sits inside, in the
   *  same idiom the search results use for the same job. A flat list of bare names is
   *  unreadable the moment two rooms both hold a "Filter", and this is the one screen where
   *  picking the wrong one silently misfiles an object instead of just showing the wrong page. */
  function optionLabel(o: MemObject): string {
    const parent = o.parent_id === null ? null : nameById.get(o.parent_id);
    return parent ? `${o.name} · ${$t('search.in-parent', { name: parent })}` : o.name;
  }

  onMount(async () => {
    if (id) {
      const o = await api<MemObject>('GET', `/objects/${id}`);
      input = toInput(o);
      priceText = centsToInput(o.purchase_price_cents);
    }
    // The descendant walk has to see the whole tree. `all=true` means "ignore nesting" only --
    // `archived` is an independent either/or filter that still applies -- so one fetch returns
    // the *unarchived* tree, and a child reachable only through an archived room is missing
    // from it. The walk would then never reach that child, offer it as a parent, and the
    // server, which walks the real table, would refuse the save with a raw 400. Both halves,
    // merged, are the whole tree.
    const [live, archived] = await Promise.all([
      api<MemObject[]>('GET', '/objects?all=true&archived=false'),
      api<MemObject[]>('GET', '/objects?all=true&archived=true'),
    ]);
    const all = [...live, ...archived];
    nameById = new Map(all.map((o) => [o.id, o.name]));
    // What is *legal* is decided against that whole tree; what is *offered* is narrower on
    // purpose. The server checks `deleted_at`, not `archived_at`, and would accept an archived
    // parent quite happily -- but filing a live object inside an archived container is not a
    // move worth offering, so it is left out here. This gap between the two rules is deliberate
    // and is not the inconsistency it looks like.
    //
    // The one archived object that does stay on the list is whichever one this object already
    // sits inside: dropping it would leave the field blank on an object that is in fact filed
    // somewhere, which reads as "top-level" and is a lie about the data.
    //
    // `live` arrives in case-insensitive name order and neither call below reorders, so the
    // offered list keeps that order.
    const alreadyInside = input.parent_id ?? null;
    parentChoices = excludingDescendants(all, editing ? Number(id) : null)
      .filter((o) => o.archived_at === null || o.id === alreadyInside);
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    input.purchase_price_cents = parseMoney(priceText);
    const bad = validate(input);
    if (bad) { error = fieldError(bad, $t); return; }
    busy = true; error = '';
    try {
      if (!input.purchase_date) input.purchase_date = null;
      const saved = editing
        ? await api<MemObject>('PATCH', `/objects/${id}`, input)
        : await api<MemObject>('POST', '/objects', input);
      if (!editing && remindReading && saved.counter_unit) {
        // Starting a month out: the reading just typed into a new object's first entry is as
        // good as today's, so asking again tomorrow would be noise. A failure here must not
        // lose the object that was just saved -- the reminder can be added from its tab.
        const start = addMonthsIso(todayIso(), 1);
        try { await api('POST', `/objects/${saved.id}/reminders`, reminderBody(readingReminder($t('reading.reminder-title'), start))); }
        catch { /* the object is saved; the reminders tab offers the same action */ }
      }
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
    {#if !editing && input.counter_unit}
      <label class="row toggle"><input type="checkbox" bind:checked={remindReading} /> {$t('object.reading-reminder')}</label>
    {/if}
    <div class="field">
      <label for="fu">{$t('object.fuel-unit')}</label>
      <select id="fu" bind:value={input.fuel_unit}>
        <option value={null}>{$t('object.counter-none')}</option>
        <option value="l">l</option>
        <option value="gal">gal</option>
        <option value="kwh">kwh</option>
      </select>
    </div>
    <div class="field">
      <label for="p">{$t('object.parent')}</label>
      <select id="p" bind:value={input.parent_id}>
        <option value={null}>{$t('object.parent-none')}</option>
        {#each parentChoices as p}<option value={p.id}>{optionLabel(p)}</option>{/each}
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
