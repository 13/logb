<script lang="ts">
  import { errorMessage } from '../lib/api-error';
  import { onMount, untrack } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import TagInput from '../lib/TagInput.svelte';
  import DateInput from '../lib/DateInput.svelte';
  import { parseWeight } from '../lib/weight';
  import { api, cancelQueuedObject, createObjectQueued, createQueued, updateQueuedObject } from '../lib/api';
  import { newOpId } from '../lib/outbox';
  import { hashToNegativeId } from '../lib/activity-form';
  import { go, back } from '../lib/router';
  import { locale, t } from '../i18n';
  import { counter, parseMoney, parseQuantity } from '../lib/format';
  import { fuelUnitLabel } from '../lib/energy';
  import { clearsPriceOn, emptyInput, formText, objectHasDetails, OBJECT_FIELD_IDS, pendingObject, toInput, validate } from '../lib/object-form';
  import { excludingDescendants } from '../lib/object-tree';
  import { fieldErrorAt, type FieldError } from '../lib/form-error';
  import { reminderBody } from '../lib/reminder-form';
  import { readingActivity } from '../lib/reading';
  import { counterStep, templateInput, templatesFor, type ReminderTemplate } from '../lib/reminder-templates';
  import { todayIso } from '../lib/format';
  import { type ResourceUnit, type MemObject, type ObjectInput, type ObjectType, type TagCount } from '../lib/types';
  import { customTypes, defaultUnit } from '../lib/type-registry';
  import { saveObjectDraft, takeObjectDraftState } from '../lib/object-draft';
  import { getCachedObject, setCachedObject } from '../lib/object-cache';
  import { user } from '../stores/session';
  import FormActions from '../lib/FormActions.svelte';
  import MoreDetails from '../lib/MoreDetails.svelte';
  import TypeTiles from '../lib/TypeTiles.svelte';
  import { revealField } from '../lib/reveal-field';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { chipClass, errorClass, hintClass, labelClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Textarea } from '$lib/components/ui/textarea/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
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
  /** Whether the user has chosen a type. A new object starts with none (the input's own 'other'
   *  is only a placeholder): "Other" as a default filed most new objects as Other. A template, a
   *  kept draft and the "+ New type…" round trip count as a choice; an edit starts chosen. */
  let typePicked = $state(untrack(() => id !== undefined));
  /** "More details": open when the object already uses any of its fields (a `?parent_id=`). */
  let moreOpen = $state(untrack(() => objectHasDetails(input)));
  /** A save refused by `validate` (or for want of a type), shown under the field it names. */
  let fieldErr = $state<FieldError | null>(null);
  const errorFor = (fid: string): string => (fieldErr?.id === fid ? fieldErr.message : '');
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
  /** Whose templates these are: they are kept per user (see ../lib/object-templates.ts). */
  const userId = () => $user?.id ?? null;
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
  /** The object being edited could not be loaded; the form holds defaults, not its data. */
  let loadFailed = $state(false);
  /** True until the draft or the object being edited has been applied to the form (`aria-busy`:
   *  the e2e helpers wait on it before opening "More details"). */
  let loading = $state(true);
  let deleteError = $state('');
  const formError = $derived(error || (fieldErr?.id === null ? fieldErr.message : ''));

  function mintTempId(): number {
    return hashToNegativeId(newOpId());
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
      const token = saveObjectDraft(currentPath, $state.snapshot(input), typePicked);
      go(`/settings/types?new=1&return=${encodeURIComponent(currentPath)}&draft=${encodeURIComponent(token)}`);
      return;
    }
    input.type = ty as ObjectType;
    if (ty === 'body' && !editing) { input.counter_unit = null; input.fuel_unit = null; input.resource_unit = null; input.resource_kind = null; input.measurement_mode = null; input.energy_price_milli = null; input.fuel_capacity_milli = null; input.monthly_target_milli = null; input.purchase_date = null; priceText = ''; energyPriceText = ''; capacityText = ''; targetText = ''; chosen = []; }
    if (input.counter_unit === null) input.counter_unit = defaultUnit(ty, $customTypes);
  }

  function pickType(ty: string) {
    typePicked = true;
    if (fieldErr?.id === 'object-type') fieldErr = null;
    setType(ty);
  }

  function applyObjectTemplate(kind: 'football' | 'electricity' | 'heating-oil' | 'water') {
    const base = emptyInput();
    if (kind === 'football') input = { ...base, name: $t('template.object-football'), type: 'other' };
    if (kind === 'electricity') input = { ...base, name: $t('template.object-electricity'), type: 'appliance', resource_kind: 'electricity', resource_unit: 'kwh', fuel_unit: 'kwh', measurement_mode: 'usage' };
    if (kind === 'heating-oil') input = { ...base, name: $t('template.object-heating-oil'), type: 'home', resource_kind: 'heating_fuel', resource_unit: 'l', fuel_unit: 'l', measurement_mode: 'usage' };
    if (kind === 'water') input = { ...base, name: $t('template.object-water'), type: 'home', resource_kind: 'water', resource_unit: 'm3', fuel_unit: null, measurement_mode: 'meter' };
    priceText = ''; energyPriceText = ''; capacityText = ''; targetText = ''; chosen = []; readingText = '';
    typePicked = true; if (objectHasDetails(input)) moreOpen = true;
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
    savedTemplates = loadObjectTemplates(userId());
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
    const typeParam = params.get('type');
    try {
      const draft = takeObjectDraftState(currentPath, draftToken);
      // The price fields round a sub-cent `energy_price_milli` to whole cents on load, like every
      // other money amount here -- see `formText` (../lib/object-form.ts).
      if (draft) {
        fill(draft.input);
        typePicked = draft.typePicked;
      } else if (id) {
        // A failed load leaves empty defaults on an EDIT url: say so, and have `submit` refuse --
        // saving that blank form would overwrite the real object with it.
        try {
          const cachedPending = Number(id) < 0 ? getCachedObject(Number(id)) : undefined;
          const o = cachedPending ?? await api<MemObject>('GET', `/objects/${id}`);
          fill(toInput(o));
        } catch (e) {
          loadFailed = true;
          error = errorMessage(e, $t);
        }
      }
      // A `type` in the query names the type just created on Types, straight from the shortcut --
      // selecting it here (through `setType`, so the counter-unit default still applies) is what
      // lands the round trip on the new type instead of back on whatever the form had before.
      if (typeParam && $customTypes.some((c) => c.key === typeParam)) pickType(typeParam);
    } finally { loading = false; }
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
    let live: MemObject[], archived: MemObject[];
    try {
      [live, archived] = await Promise.all([
        api<MemObject[]>('GET', '/objects?all=true&archived=false'),
        api<MemObject[]>('GET', '/objects?all=true&archived=true'),
      ]);
    } catch (e) {
      if (!error) error = errorMessage(e, $t);
      return;
    }
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

  /** The whole form, from one input: the input itself and the text fields shown for it. */
  function fill(next: ObjectInput) {
    input = next;
    if (objectHasDetails(next)) moreOpen = true;
    ({ priceText, energyPriceText, capacityText, targetText } = formText(next));
  }

  function applySavedTemplate(template: SavedObjectTemplate) {
    fill({ ...structuredClone(template.input), name: template.name });
    typePicked = true;
  }

  async function reject(key: string) {
    const at = fieldErrorAt(key, $t, OBJECT_FIELD_IDS);
    fieldErr = at;
    if (at.id !== null && !(await revealField(at.id))) fieldErr = { id: null, message: at.message };
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    fieldErr = null; error = '';
    if (loadFailed) { error = $t('object.not-loaded'); return; }
    if (!typePicked) { await reject('object.type'); return; }
    input.purchase_price_cents = parseMoney(priceText);
    // Cents x1000, the same scale as `cost_per_counter_milli`; empty (or no fuel unit at all,
    // which hides the field) means no price. `Number.isNaN(cents) * 1000` stays `NaN`, so an
    // unparseable price still reaches `validate` below rather than being silently swallowed.
    const energyPriceCents = parseMoney(energyPriceText);
    input.energy_price_milli = input.resource_unit === null || input.resource_unit === undefined || energyPriceCents === null ? null : energyPriceCents * 1000;
    input.fuel_capacity_milli = input.resource_kind === 'heating_fuel' && (input.resource_unit === 'l' || input.resource_unit === 'gal') ? parseQuantity(capacityText) : null;
    input.monthly_target_milli = input.resource_kind ? parseQuantity(targetText) : null;
    const bad = validate(input);
    if (bad) { await reject(bad); return; }
    const grams = !editing && input.type === 'body' && startingWeight.trim() ? parseWeight(startingWeight, input.weight_unit ?? 'kg') : null;
    if (grams !== null && !Number.isFinite(grams)) { await reject('weight.invalid'); return; }
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
        setCachedObject(tempId, pendingObject($state.snapshot(input) as ObjectInput, { tempId, now: new Date().toISOString() }));
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
    } catch (err) { error = errorMessage(err, $t); } finally { busy = false; }
  }

  async function remove() {
    if (busy || !confirm($t('nav.confirm-delete'))) return;
    busy = true; deleteError = '';
    try {
      if (Number(id) < 0) await cancelQueuedObject(Number(id));
      else await api('DELETE', `/objects/${id}`);
      go('/', true);
    } catch (e) {
      deleteError = errorMessage(e, $t);
    } finally { busy = false; }
  }
</script>

<main>
  <TopBar title={editing ? $t('object.edit') : $t('object.new')} backTo={editing ? `/objects/${id}` : '/'} />
  <form onsubmit={submit} aria-busy={loading} class="m-0 flex w-full max-w-[40rem] flex-col gap-5">
    <TypeTiles value={input.type} picked={typePicked} error={errorFor('object-type')} onpick={pickType} onnewtype={() => setType(NEW_TYPE)} />

    {#if !editing}
      <section aria-labelledby="object-templates-label" class="flex flex-col gap-1.5">
        <h2 id="object-templates-label" class={labelClass}>{$t('object.quick-templates')}</h2>
        <div class="-mx-1 flex gap-2 overflow-x-auto px-1 py-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          <button type="button" data-slot="template-chip" class={chipClass} onclick={() => applyObjectTemplate('football')}>{$t('template.object-football')}</button>
          <button type="button" data-slot="template-chip" class={chipClass} onclick={() => applyObjectTemplate('electricity')}>{$t('template.object-electricity')}</button>
          <button type="button" data-slot="template-chip" class={chipClass} onclick={() => applyObjectTemplate('heating-oil')}>{$t('template.object-heating-oil')}</button>
          <button type="button" data-slot="template-chip" class={chipClass} onclick={() => applyObjectTemplate('water')}>{$t('template.object-water')}</button>
          {#each savedTemplates as template (template.id)}
            <span class="inline-flex shrink-0 items-center rounded-full border border-border bg-card">
              <button type="button" data-slot="template-chip" class="min-h-11 cursor-pointer rounded-l-full px-3 text-sm whitespace-nowrap text-foreground hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring"
                      onclick={() => applySavedTemplate(template)}>{template.name}</button>
              <button type="button" data-slot="template-remove" aria-label={$t('template.remove', { name: template.name })}
                      class="grid size-11 cursor-pointer place-items-center rounded-r-full text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring"
                      onclick={() => (savedTemplates = removeObjectTemplate(userId(), template.id))}>×</button>
            </span>
          {/each}
        </div>
      </section>
    {/if}

    <Field id="n" label={$t('object.name')} error={errorFor('n')}><Input bind:value={input.name} required /></Field>

    {#if (typePicked || editing) && (input.type !== 'body' || editing)}
      <Field id="u" label={$t('object.counter')}>
        <NativeSelect bind:value={input.counter_unit}>
          <option value={null}>{$t('object.counter-none')}</option>
          <option value="km">{$t('object.counter-km')}</option>
          <option value="mi">{$t('object.counter-mi')}</option>
          <option value="h">{$t('object.counter-h')}</option>
        </NativeSelect>
      </Field>
      {#if offeredTemplates.length > 0}
        <fieldset class="m-0 flex min-w-0 flex-col gap-1 border-0 p-0" aria-describedby="templates-hint">
          <legend class={`${labelClass} mb-1 p-0`}>{$t('object.templates')}</legend>
          <p id="templates-hint" class={hintClass}>{$t('object.templates-hint')}</p>
          {#each offeredTemplates as tp (tp.id)}
            <CheckField id={`template-${tp.id}`} label={$t(tp.title)} detail={schedule(tp)}
                        bind:checked={() => chosen.includes(tp.id), (on) => (chosen = on ? [...chosen, tp.id] : chosen.filter((c) => c !== tp.id))} />
          {/each}
          {#if needsReading}
            <Field id="cr" label={$t('object.current-reading', { unit: input.counter_unit ?? '' })} hint={$t('object.current-reading-hint')} class="mt-2">
              <Input type="number" inputmode="numeric" min="0" step="1" bind:value={readingText} />
            </Field>
          {/if}
        </fieldset>
      {/if}
    {/if}

    {#if typePicked && input.type === 'body'}
      <Field id="wu" label={$t('weight.unit')}>
        <NativeSelect bind:value={input.weight_unit}><option value="kg">kg</option><option value="lb">lb</option></NativeSelect>
      </Field>
      {#if !editing}
        <Field id="sw" label={$t('weight.starting')}><Input type="text" inputmode="decimal" bind:value={startingWeight} /></Field>
        {#if startingWeight}
          <Field id="wd" label={$t('activity.date')}><DateInput id="wd" bind:value={weightDate} max={todayIso()} required /></Field>
        {/if}
      {/if}
    {/if}

    <MoreDetails bind:open={moreOpen}>
      {#if input.type !== 'body' || editing}
        <Field id="resource-kind" label={$t('object.resource-kind')}>
          <NativeSelect bind:value={input.resource_kind}>
            <option value={null}>{$t('object.counter-none')}</option>
            <option value="electricity">{$t('resource.electricity')}</option>
            <option value="heating_fuel">{$t('resource.heating-fuel')}</option>
            <option value="vehicle_fuel">{$t('resource.vehicle-fuel')}</option>
            <option value="water">{$t('resource.water')}</option>
          </NativeSelect>
        </Field>
        {#if input.resource_kind}
          <Field id="fu" label={$t('object.resource-unit')}>
            <NativeSelect bind:value={() => input.resource_unit ?? null, setResourceUnit}>
              <option value={null}>{$t('object.counter-none')}</option>
              <option value="l">l</option>
              <option value="gal">gal</option>
              <option value="kwh">{fuelUnitLabel('kwh')}</option>
              <option value="m3">m³</option>
            </NativeSelect>
          </Field>
          {#if input.resource_unit}
            <Field id="ep" label={$t('object.energy-price', { unit: fuelUnitLabel(input.resource_unit) })} error={errorFor('ep')}>
              <Input type="text" inputmode="decimal" bind:value={energyPriceText} />
            </Field>
          {/if}
          {#if input.resource_kind === 'water'}
            <Field id="measurement-mode" label={$t('object.measurement-mode')}>
              <NativeSelect bind:value={input.measurement_mode}>
                <option value="meter">{$t('water.mode-meter')}</option>
                <option value="usage">{$t('water.mode-usage')}</option>
              </NativeSelect>
            </Field>
          {/if}
          <Field id="monthly-target" label={$t('object.monthly-target', { unit: fuelUnitLabel(input.resource_unit ?? null) })}>
            <Input type="text" inputmode="decimal" bind:value={targetText} />
          </Field>
          {#if input.resource_kind === 'heating_fuel' && (input.resource_unit === 'l' || input.resource_unit === 'gal')}
            <div class="grid grid-cols-2 gap-3">
              <Field id="capacity" label={$t('object.fuel-capacity', { unit: fuelUnitLabel(input.resource_unit) })} error={errorFor('capacity')}>
                <Input type="text" inputmode="decimal" bind:value={capacityText} />
              </Field>
              <Field id="low-level" label={$t('object.low-level')}>
                <Input type="number" min="0" max="100" bind:value={input.low_level_pct} />
              </Field>
            </div>
          {/if}
        {/if}
      {/if}
      <Field id="p" label={$t('object.parent')}>
        <NativeSelect bind:value={input.parent_id}>
          <option value={null}>{$t('object.parent-none')}</option>
          {#each parentChoices as p (p.id)}<option value={p.id}>{optionLabel(p)}</option>{/each}
        </NativeSelect>
      </Field>
      <Field id="d" label={$t('object.description')}><Textarea bind:value={input.description} /></Field>
      <TagInput bind:tags={() => input.tags ?? [], (v) => (input.tags = v)} suggestions={tagCounts} label={$t('tags.label')} id="tags" />
      {#if input.type !== 'body' || editing}
        <div class="grid grid-cols-2 gap-3">
          <Field id="pd" label={$t('object.purchase-date')}>
            <DateInput id="pd" bind:value={() => input.purchase_date ?? '', (v) => (input.purchase_date = v || null)} />
          </Field>
          <Field id="pp" label={$t('object.purchase-price')} error={errorFor('pp')}>
            <Input type="text" inputmode="decimal" bind:value={priceText} />
          </Field>
        </div>
      {/if}
      {#if editing}
        <CheckField id="archive" label={$t('object.archive')} hint={$t('object.archived-hint')} bind:checked={input.archived} />
      {/if}
      <CheckField id="private" label={$t('object.private')} hint={$t('object.private-hint')} bind:checked={input.private} />
    </MoreDetails>

    {#if !editing && input.name.trim()}
      <Button variant="outline" class="h-12 w-fit" onclick={() => (savedTemplates = saveObjectTemplate(userId(), $state.snapshot(input)))}>{$t('template.save')}</Button>
    {/if}

    <FormActions {busy} error={formError} oncancel={() => back(editing ? `/objects/${id}` : '/')} />
  </form>
  {#if editing}
    <section aria-labelledby="object-delete" class="mt-8 flex max-w-[40rem] flex-col gap-2 border-t border-border pt-4">
      <h2 id="object-delete" class={sectionHeadingClass}>{$t('object.delete')}</h2>
      <p class={hintClass}>{$t('object.delete-hint')}</p>
      <Button variant="destructive" class="h-12 w-fit" disabled={busy} onclick={remove}>{$t('object.delete')}</Button>
      {#if deleteError}<p role="alert" class={errorClass}>{deleteError}</p>{/if}
    </section>
  {/if}
</main>
