<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import TagInput from '../lib/TagInput.svelte';
  import DateInput from '../lib/DateInput.svelte';
  import { parseWeight } from '../lib/weight';
  import { api, cancelQueuedObject, createObjectQueued, createQueued, updateQueuedObject } from '../lib/api';
  import { newOpId } from '../lib/outbox';
  import { go, back } from '../lib/router';
  import { locale, t } from '../i18n';
  import { centsToInput, counter, parseMoney, parseQuantity } from '../lib/format';
  import { fuelUnitLabel } from '../lib/energy';
  import { clearsPriceOn, emptyInput, toInput, validate } from '../lib/object-form';
  import { excludingDescendants } from '../lib/object-tree';
  import { fieldError } from '../lib/form-error';
  import { reminderBody } from '../lib/reminder-form';
  import { readingActivity } from '../lib/reading';
  import { counterStep, templateInput, templatesFor, type ReminderTemplate } from '../lib/reminder-templates';
  import { todayIso } from '../lib/format';
  import { OBJECT_TYPES, type ResourceUnit, type MemObject, type ObjectInput, type ObjectType, type TagCount } from '../lib/types';
  import { customTypes, defaultUnit, typesLoaded } from '../lib/type-registry';
  import { saveObjectDraft, takeObjectDraft } from '../lib/object-draft';
  import { getCachedObject, setCachedObject } from '../lib/object-cache';
  import { loadObjectTemplates, removeObjectTemplate, saveObjectTemplate, type SavedObjectTemplate } from '../lib/object-templates';

  let { id }: { id?: string } = $props();
  const editing = $derived(id !== undefined);
  /** This route's own path, exactly as `App.svelte` registers it -- the key a draft is kept
   *  under across the "+ New type…" round trip, and the `return` the shortcut sends Types. */
  const currentPath = $derived(editing ? `/objects/${id}/edit` : '/objects/new');
  const presetParentId = new URLSearchParams(location.search).get('parent_id');
  let input = $state<ObjectInput>(
    untrack(() => (id === undefined ? { ...emptyInput(), parent_id: presetParentId ? Number(presetParentId) : null } : emptyInput())),
  );
  let startingWeight = $state('');
  let weightDate = $state(todayIso());
  let createdId: number | null = null;
  let priceText = $state('');
  /** The "Price per {unit}" field, next to the fuel unit -- kept as its own text state like
   *  `priceText`, parsed at submit time. */
  let energyPriceText = $state('');
  let capacityText = $state('');
  let targetText = $state('');
  let savedTemplates = $state<SavedObjectTemplate[]>([]);
  /** Ticked template ids. Opt-in, never automatic: a reminder nobody asked for is the kind that
   *  gets muted. */
  let chosen = $state<string[]>([]);
  let readingText = $state('');
  const offeredTemplates = $derived(editing ? [] : templatesFor(input.type, input.counter_unit));
  const ticked = $derived(offeredTemplates.filter((tp) => chosen.includes(tp.id)));
  /** A distance-based reminder is only right from where the counter is now. */
  const needsReading = $derived(ticked.some((tp) => counterStep(tp, input.counter_unit) !== null));

  /** "every 15,000 km or 12 months", in the reader's language and number format. */
  function schedule(tp: ReminderTemplate): string {
    if (tp.reading) return $t('reminder.every-month');
    const parts: string[] = [];
    const step = counterStep(tp, input.counter_unit);
    if (step !== null) parts.push(counter(step, input.counter_unit, $locale));
    if (tp.months !== undefined) parts.push($t('template.months', { n: tp.months }));
    return $t('template.every', { what: parts.join(` ${$t('template.or')} `) });
  }
  let error = $state('');
  let busy = $state(false);

  function mintTempId(): number {
    const value = newOpId();
    let hash = 0;
    for (let i = 0; i < value.length; i++) hash = (hash * 31 + value.charCodeAt(i)) | 0;
    return -(Math.abs(hash) || 1);
  }

  /** The type select's own last option: picking it does not choose a type at all, it detours to
   *  Types to make one, keeping this form's input for when it comes back. Not a legal
   *  `ObjectType` -- kept out of that type on purpose, so nothing downstream can mistake it for
   *  a real selection. */
  const NEW_TYPE = '__new_type';

  /** An own type knows the counter it usually has (an e-scooter counts km), so choosing it fills
   *  in the unit -- but only into an empty one: a unit the user already picked is theirs. */
  function setType(ty: string) {
    if (ty === NEW_TYPE) {
      const token = saveObjectDraft(currentPath, $state.snapshot(input));
      go(`/settings/types?new=1&return=${encodeURIComponent(currentPath)}&draft=${encodeURIComponent(token)}`);
      return;
    }
    input.type = ty as ObjectType;
    if (ty === 'body' && !editing) { input.counter_unit = null; input.fuel_unit = null; input.resource_unit = null; input.resource_kind = null; input.measurement_mode = null; input.energy_price_milli = null; input.fuel_capacity_milli = null; input.monthly_target_milli = null; input.purchase_date = null; priceText = ''; energyPriceText = ''; capacityText = ''; targetText = ''; chosen = []; }
    if (input.counter_unit === null) input.counter_unit = defaultUnit(ty, $customTypes);
  }

  function applyObjectTemplate(kind: 'football' | 'electricity' | 'heating-oil' | 'water') {
    const base = emptyInput();
    if (kind === 'football') input = { ...base, name: $t('template.object-football'), type: 'other' };
    if (kind === 'electricity') input = { ...base, name: $t('template.object-electricity'), type: 'appliance', resource_kind: 'electricity', resource_unit: 'kwh', fuel_unit: 'kwh', measurement_mode: 'usage' };
    if (kind === 'heating-oil') input = { ...base, name: $t('template.object-heating-oil'), type: 'home', resource_kind: 'heating_fuel', resource_unit: 'l', fuel_unit: 'l', measurement_mode: 'usage' };
    if (kind === 'water') input = { ...base, name: $t('template.object-water'), type: 'home', resource_kind: 'water', resource_unit: 'm3', fuel_unit: null, measurement_mode: 'meter' };
    priceText = ''; energyPriceText = ''; capacityText = ''; targetText = ''; chosen = []; readingText = '';
  }

  /** A price kept from the previous fuel unit would misread as the new one (a €/kWh figure
   *  surviving a switch to litres) -- `clearsPriceOn` (../lib/object-form.ts) says so on any
   *  actual change, and this is a UI-side clear only: the server still allows the stale
   *  combination. */
  function setResourceUnit(next: ResourceUnit) {
    if (clearsPriceOn(input.fuel_unit, next === 'm3' ? null : next)) energyPriceText = '';
    if (next !== 'l' && next !== 'gal') capacityText = '';
    input.resource_unit = next;
    input.fuel_unit = next === 'm3' ? null : next;
  }
  /** An object whose own type is gone (deleted elsewhere, not synced here yet) still has to show
   *  something selected, or the select would sit on a blank and look like it lost the type. */
  const missingType = $derived(input.type.startsWith('custom:') && !$customTypes.some((c) => c.key === input.type));
  /** Tags already in use, offered while typing one. */
  let tagCounts = $state<TagCount[]>([]);
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
    savedTemplates = loadObjectTemplates();
    // Not awaited, and a failure is ignored: suggestions are a convenience, and the form must not
    // wait for them or lose its object load over them.
    api<TagCount[]>('GET', '/tags').then((list) => (tagCounts = list), () => {});
    const params = new URLSearchParams(location.search);
    // The token in `draft=` is what makes this restore safe: only the one mount that just made
    // the round trip -- Types sending back exactly the token this form minted -- may claim a
    // kept draft. A plain later visit to this same route (no token, a stale one from history,
    // the back button) carries none that matches, and the draft is discarded rather than shown
    // to whoever mounts next -- see `takeObjectDraft`.
    const draftToken = params.get('draft');
    const draft = takeObjectDraft(currentPath, draftToken);
    // `energy_price_milli` is cents x1000 -- sub-cent precision the *field* can carry (a price
    // agreed to three decimal places) -- but this form's price input is deliberately the same
    // whole-cent text field every other money amount here uses (`centsToInput`/`parseMoney`, per
    // the design spec: "parsed like other money input"), so a price entered with a fractional
    // cent is rounded to the nearest whole one on every load, same as `purchase_price_cents`
    // already is. Consistent with the rest of the app rather than a precision loss unique to
    // this field.
    if (draft) {
      input = draft;
      priceText = centsToInput(draft.purchase_price_cents);
      energyPriceText = centsToInput(draft.energy_price_milli == null ? null : Math.round(draft.energy_price_milli / 1000));
      capacityText = draft.fuel_capacity_milli == null ? '' : String(draft.fuel_capacity_milli / 1000);
      targetText = draft.monthly_target_milli == null ? '' : String(draft.monthly_target_milli / 1000);
    } else if (id) {
      const cachedPending = Number(id) < 0 ? getCachedObject(Number(id)) : undefined;
      const o = cachedPending ?? await api<MemObject>('GET', `/objects/${id}`);
      input = toInput(o);
      priceText = centsToInput(o.purchase_price_cents);
      energyPriceText = centsToInput(o.energy_price_milli === null ? null : Math.round(o.energy_price_milli / 1000));
      capacityText = o.fuel_capacity_milli == null ? '' : String(o.fuel_capacity_milli / 1000);
      targetText = o.monthly_target_milli == null ? '' : String(o.monthly_target_milli / 1000);
    }
    // A `type` in the query names the type just created on Types, straight from the shortcut --
    // selecting it here (through `setType`, so the counter-unit default still applies) is what
    // lands the round trip on the new type instead of back on whatever the form had before.
    const typeParam = params.get('type');
    if (typeParam && $customTypes.some((c) => c.key === typeParam)) setType(typeParam);
    // Both are one-shot: a reload of this exact URL must not re-apply `type` over a draft that
    // is already consumed, nor offer up a `draft` token a second time (see `takeObjectDraft`'s
    // "used at most once"). Mirrors ObjectDetail's `?tag=` -- strip what was just consumed, keep
    // whatever else genuinely belongs in the address.
    if (typeParam !== null || draftToken !== null) history.replaceState(null, '', currentPath);
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

  function applySavedTemplate(template: SavedObjectTemplate) {
    input = structuredClone(template.input); input.name = template.name;
    priceText = centsToInput(input.purchase_price_cents); energyPriceText = centsToInput(input.energy_price_milli == null ? null : Math.round(input.energy_price_milli / 1000));
    capacityText = input.fuel_capacity_milli == null ? '' : String(input.fuel_capacity_milli / 1000);
    targetText = input.monthly_target_milli == null ? '' : String(input.monthly_target_milli / 1000);
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    input.purchase_price_cents = parseMoney(priceText);
    // Cents x1000, the same scale as `cost_per_counter_milli`; empty (or no fuel unit at all,
    // which hides the field) means no price. `Number.isNaN(cents) * 1000` stays `NaN`, so an
    // unparseable price still reaches `validate` below rather than being silently swallowed.
    const energyPriceCents = parseMoney(energyPriceText);
    input.energy_price_milli = input.resource_unit === null || input.resource_unit === undefined || energyPriceCents === null ? null : energyPriceCents * 1000;
    input.fuel_capacity_milli = input.resource_kind === 'heating_fuel' && (input.resource_unit === 'l' || input.resource_unit === 'gal') ? parseQuantity(capacityText) : null;
    input.monthly_target_milli = input.resource_kind ? parseQuantity(targetText) : null;
    const bad = validate(input);
    if (bad) { error = fieldError(bad, $t); return; }
    const grams = !editing && input.type === 'body' && startingWeight.trim() ? parseWeight(startingWeight, input.weight_unit ?? 'kg') : null;
    if (grams !== null && !Number.isFinite(grams)) { error = $t('weight.invalid'); return; }
    busy = true; error = '';
    try {
      if (!input.purchase_date) input.purchase_date = null;
      if (editing && Number(id) < 0) {
        const body = $state.snapshot(input) as unknown as Record<string, unknown>;
        if (!await updateQueuedObject(Number(id), body)) throw new Error('object.pending-lost');
        const cached = getCachedObject(Number(id));
        if (cached) setCachedObject(Number(id), { ...cached, ...input, private: input.private ? 1 : 0, pending: true } as MemObject);
        go('/', true);
        return;
      }
      const tempId = mintTempId();
      const saved = createdId !== null ? await api<MemObject>('PATCH', `/objects/${createdId}`, input) : editing
        ? await api<MemObject>('PATCH', `/objects/${id}`, input)
        : await createObjectQueued<MemObject>($state.snapshot(input) as unknown as Record<string, unknown>, tempId);
      if (!saved) {
        const now = new Date().toISOString();
        setCachedObject(tempId, {
          id: tempId, user_id: 0, ...$state.snapshot(input), fuel_unit: input.fuel_unit,
          archived_at: null, cover_attachment_id: null, cover_file_id: null, created_at: now, updated_at: now,
          ancestors: [], tags: [...(input.tags ?? [])], private: input.private ? 1 : 0,
          stats: { total_cost_cents: 0, activity_count: 0, current_counter: null, latest_weight_grams: null, latest_weight_date: null,
            due_reminder_count: 0, last_reading_date: null, last_activity_date: null, counter_per_day_milli: null }, pending: true,
        } as MemObject);
        go(`/objects/${tempId}`, true);
        return;
      }
      if (!editing) createdId = saved.id;
      if (grams !== null) {
        await createQueued(`/objects/${saved.id}/activities`, {date: weightDate, category: 'weight', title: $t('cat.weight'), notes: '', weight_grams: grams, counter_value: null, cost_cents: null, quantity_milli: null});
        startingWeight = '';
      }
      if (!editing && ticked.length > 0) {
        const today = todayIso();
        const current = String(readingText).trim() === '' ? null : Number(readingText);
        const reading = current !== null && Number.isInteger(current) && current >= 0 ? current : null;
        // A failure here must not lose the object that was just saved: every one of these can
        // be added from the object's own tabs afterwards.
        try {
          if (reading !== null && saved.counter_unit) {
            await api('POST', `/objects/${saved.id}/activities`, readingActivity(reading, today, $t('reading.entry-title')));
          }
          for (const tp of ticked) {
            const body = templateInput(tp, { title: $t(tp.title), unit: saved.counter_unit, currentReading: reading, today });
            if (body) await api('POST', `/objects/${saved.id}/reminders`, reminderBody(body));
          }
        } catch { /* the object is saved; its reminders tab offers the same */ }
      }
      go(`/objects/${saved.id}`, true);
    } catch (err) { error = (err as Error).message; } finally { busy = false; }
  }

  async function remove() {
    if (!confirm($t('nav.confirm-delete'))) return;
    if (Number(id) < 0) { await cancelQueuedObject(Number(id)); go('/', true); return; }
    await api('DELETE', `/objects/${id}`);
    go('/', true);
  }
</script>

<main>
  <TopBar title={editing ? $t('object.edit') : $t('object.new')} backTo={editing ? `/objects/${id}` : '/'} />
  {#if !editing}
    <div class="chips" aria-label={$t('object.quick-templates')}>
      <button type="button" class="chip" onclick={() => applyObjectTemplate('football')}>{$t('template.object-football')}</button>
      <button type="button" class="chip" onclick={() => applyObjectTemplate('electricity')}>{$t('template.object-electricity')}</button>
      <button type="button" class="chip" onclick={() => applyObjectTemplate('heating-oil')}>{$t('template.object-heating-oil')}</button>
      <button type="button" class="chip" onclick={() => applyObjectTemplate('water')}>{$t('template.object-water')}</button>
      {#each savedTemplates as template (template.id)}
        <button type="button" class="chip" onclick={() => applySavedTemplate(template)}>{template.name}</button>
        <button type="button" class="chip" aria-label={$t('template.remove', { name: template.name })} onclick={() => (savedTemplates = removeObjectTemplate(template.id))}>×</button>
      {/each}
    </div>
  {/if}
  <form onsubmit={submit}>
    <div class="field"><label for="n">{$t('object.name')}</label><input id="n" bind:value={input.name} required /></div>
    <div class="field">
      <label for="c">{$t('object.type')}</label>
      <select id="c" bind:value={() => input.type, setType}>
        {#each OBJECT_TYPES as ty}<option value={ty}>{$t(`type.${ty}`)}</option>{/each}
        {#if $customTypes.length > 0}
          <optgroup label={$t('types.yours')}>
            {#each $customTypes as ct (ct.key)}<option value={ct.key}>{ct.name}</option>{/each}
          </optgroup>
        {/if}
        {#if missingType}<option value={input.type}>{$typesLoaded ? $t('types.unknown') : $t('types.loading')}</option>{/if}
        <option value={NEW_TYPE}>{$t('types.new-from-form')}</option>
      </select>
    </div>
    {#if input.type !== 'body' || editing}
    <div class="field">
      <label for="u">{$t('object.counter')}</label>
      <select id="u" bind:value={input.counter_unit}>
        <option value={null}>{$t('object.counter-none')}</option>
        <option value="km">{$t('object.counter-km')}</option>
        <option value="mi">{$t('object.counter-mi')}</option>
        <option value="h">{$t('object.counter-h')}</option>
      </select>
    </div>
    {#if offeredTemplates.length > 0}
      <fieldset class="templates">
        <legend>{$t('object.templates')}</legend>
        <p class="hint">{$t('object.templates-hint')}</p>
        {#each offeredTemplates as tp (tp.id)}
          <label class="row toggle">
            <input type="checkbox" value={tp.id} bind:group={chosen} />
            <span>{$t(tp.title)} <span class="muted">· {schedule(tp)}</span></span>
          </label>
        {/each}
        {#if needsReading}
          <div class="field">
            <label for="cr">{$t('object.current-reading', { unit: input.counter_unit ?? '' })}</label>
            <input id="cr" type="number" inputmode="numeric" min="0" step="1" bind:value={readingText} />
            <span class="hint">{$t('object.current-reading-hint')}</span>
          </div>
        {/if}
      </fieldset>
    {/if}
    <div class="field">
      <label for="resource-kind">{$t('object.resource-kind')}</label>
      <select id="resource-kind" bind:value={input.resource_kind}>
        <option value={null}>{$t('object.counter-none')}</option>
        <option value="electricity">{$t('resource.electricity')}</option>
        <option value="heating_fuel">{$t('resource.heating-fuel')}</option>
        <option value="vehicle_fuel">{$t('resource.vehicle-fuel')}</option>
        <option value="water">{$t('resource.water')}</option>
      </select>
    </div>
    {#if input.resource_kind}
    <div class="field">
      <label for="fu">{$t('object.resource-unit')}</label>
      <select id="fu" bind:value={() => input.resource_unit ?? null, setResourceUnit}>
        <option value={null}>{$t('object.counter-none')}</option>
        <option value="l">l</option>
        <option value="gal">gal</option>
        <option value="kwh">{fuelUnitLabel('kwh')}</option>
        <option value="m3">m³</option>
      </select>
    </div>
    {#if input.resource_unit}
      <div class="field">
        <label for="ep">{$t('object.energy-price', { unit: fuelUnitLabel(input.resource_unit) })}</label>
        <input id="ep" type="text" inputmode="decimal" bind:value={energyPriceText} />
      </div>
    {/if}
    {#if input.resource_kind === 'water'}
      <div class="field"><label for="measurement-mode">{$t('object.measurement-mode')}</label><select id="measurement-mode" bind:value={input.measurement_mode}><option value="meter">{$t('water.mode-meter')}</option><option value="usage">{$t('water.mode-usage')}</option></select></div>
    {/if}
    <div class="field"><label for="monthly-target">{$t('object.monthly-target', { unit: fuelUnitLabel(input.resource_unit ?? null) })}</label><input id="monthly-target" type="text" inputmode="decimal" bind:value={targetText} /></div>
    {#if input.resource_kind === 'heating_fuel' && (input.resource_unit === 'l' || input.resource_unit === 'gal')}
      <div class="field">
        <label for="capacity">{$t('object.fuel-capacity', { unit: fuelUnitLabel(input.resource_unit) })}</label>
        <input id="capacity" type="text" inputmode="decimal" bind:value={capacityText} />
      </div>
      <div class="field"><label for="low-level">{$t('object.low-level')}</label><input id="low-level" type="number" min="0" max="100" bind:value={input.low_level_pct} /></div>
    {/if}
    {/if}
    {/if}
    {#if input.type === 'body'}
      <div class="field"><label for="wu">{$t('weight.unit')}</label><select id="wu" bind:value={input.weight_unit}><option value="kg">kg</option><option value="lb">lb</option></select></div>
      {#if !editing}
        <div class="field"><label for="sw">{$t('weight.starting')}</label><input id="sw" type="text" inputmode="decimal" bind:value={startingWeight} /></div>
        {#if startingWeight}<div class="field"><label for="wd">{$t('activity.date')}</label><DateInput id="wd" bind:value={weightDate} max={todayIso()} required /></div>{/if}
      {/if}
    {/if}
    <div class="field">
      <label for="p">{$t('object.parent')}</label>
      <select id="p" bind:value={input.parent_id}>
        <option value={null}>{$t('object.parent-none')}</option>
        {#each parentChoices as p}<option value={p.id}>{optionLabel(p)}</option>{/each}
      </select>
    </div>
    <div class="field"><label for="d">{$t('object.description')}</label><textarea id="d" bind:value={input.description}></textarea></div>
    <TagInput bind:tags={() => input.tags ?? [], (v) => (input.tags = v)} suggestions={tagCounts} label={$t('tags.label')} id="tags" />
    {#if input.type !== 'body' || editing}
    <div class="row">
      <div class="field"><label for="pd">{$t('object.purchase-date')}</label><DateInput id="pd" bind:value={() => input.purchase_date ?? '', (v) => (input.purchase_date = v || null)} /></div>
      <div class="field"><label for="pp">{$t('object.purchase-price')}</label><input id="pp" type="text" inputmode="decimal" bind:value={priceText} /></div>
    </div>
    {/if}
    {#if editing}
      <label class="row toggle"><input type="checkbox" bind:checked={input.archived} /> {$t('object.archive')}</label>
      <p class="hint">{$t('object.archived-hint')}</p>
    {/if}
    <label class="row toggle"><input type="checkbox" bind:checked={input.private} /> {$t('object.private')}</label>
    <p class="hint">{$t('object.private-hint')}</p>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="row actions">
      {#if !editing && input.name.trim()}<button type="button" class="ghost" onclick={() => (savedTemplates = saveObjectTemplate($state.snapshot(input)))}>{$t('template.save')}</button>{/if}
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
  .actions { margin-top: var(--space-2); }
  .templates { border: none; padding: 0; margin: 0 0 var(--space-3); display: flex; flex-direction: column; gap: var(--space-2); }
  .templates legend { font-size: var(--text-sm); color: var(--muted); padding: 0; margin-bottom: var(--space-1); }
  .templates .hint { margin: 0; }
</style>
