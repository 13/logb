<script lang="ts">
  import { errorMessage } from '../lib/api-error';
  import { formatWeight } from '../lib/weight';
  import type { WeightUnit } from '../lib/types';
  import { onMount, untrack } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import FilePicker from '../lib/FilePicker.svelte';
  import Icon from '../lib/Icon.svelte';
  import TagInput from '../lib/TagInput.svelte';
  import DateInput from '../lib/DateInput.svelte';
  import { api, cancelQueuedActivity, createQueued, fileUrl, isRejection, onOutboxFlushed, updateQueued, updateQueuedActivity } from '../lib/api';
  import { newOpId, serialize } from '../lib/outbox';
  import { getCachedObject, setCachedObject } from '../lib/object-cache';
  import { go, back } from '../lib/router';
  import { centsToInput, counter as fmtCounter, fmtDate, todayIso } from '../lib/format';
  import { dateFormat } from '../stores/date-format';
  import { activityTitle, activityToFormText, buildActivityInput, changeWeightUnitState, counterBelowLast, emptyActivity, firstExifDate, hashToNegativeId, optimisticActivity, parseCategoryParam, resolveCategory, suggestionsFor, toActivityInput, validateActivity, weightDeviates, activityHasDetails, ACTIVITY_FIELD_IDS } from '../lib/activity-form';
  import { fieldError, fieldErrorAt, type FieldError } from '../lib/form-error';
  import FormActions from '../lib/FormActions.svelte';
  import MoreDetails from '../lib/MoreDetails.svelte';
  import { revealField } from '../lib/reveal-field';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { chipClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { Textarea } from '$lib/components/ui/textarea/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { Button } from '$lib/components/ui/button/index.js';
  import { linkTripDistance, linkTripEnd, linkTripStart, type TripLink } from '../lib/trip';
  import { energyLabelKey, fuelUnitLabel } from '../lib/energy';
  import { categoriesFor, customTypes } from '../lib/type-registry';
  import { locale, t } from '../i18n';
  import { type Activity, type Attachment, type Category, type MemObject, type ActivityInput, type TagCount, type TitleSuggestion, type TripPlaces } from '../lib/types';

  let { id, aid }: { id: string; aid?: string } = $props();
  const oid = $derived(Number(id));
  const editing = $derived(aid !== undefined);

  let object = $state<MemObject | null>(null);
  let input = $state<ActivityInput>(emptyActivity());
  let weightText = $state('');
  let weightUnit = $state<WeightUnit>('kg');
  let originalWeightText = '';
  let originalWeightUnit: WeightUnit = 'kg';
  let originalWeight: number | null = null;
  let costText = $state('');
  let counterText = $state('');
  let quantityText = $state('');
  let meterReadingText = $state('');
  /** The "Charged full" checkbox, fuel-only. Ticked by default on a new entry -- most charges
   *  (or fill-ups) do top up -- and read back from the loaded row's own `charged_full` on an
   *  edit, same as every other fuel-only field below. */
  let chargedFull = $state(true);
  // Trip-only text fields: `from_place`/`to_place` are nullable strings, so (unlike `input.title`
  // or `.notes`) they cannot be bound to a text input directly without the field showing the
  // literal word "null" the moment the category becomes trip -- same reason `counterText` above
  // is its own state rather than a direct bind to the nullable `counter_value`. `durationText`
  // holds whatever the user typed (`parseDuration` in `buildInput` turns it into minutes only at
  // save time), so an in-progress "1:1" is never clobbered mid-edit.
  let fromText = $state('');
  let toText = $state('');
  let durationText = $state('');
  /** The trip Distance field. Not part of `ActivityInput` -- only start/end travel to the server
   *  (see the type's own doc comment) -- so it lives here, linked to Start/End by the on*Change
   *  handlers below exactly as the spec describes. */
  let distance = $state<number | null>(null);
  let tripPlaces = $state<TripPlaces>({ from: [], to: [] });
  let attachments = $state<Attachment[]>([]);
  let saved = $state<Activity | null>(null);
  let error = $state('');
  let busy = $state(false);
  /** A save refused by `validateActivity`, shown under the field it names. */
  let fieldErr = $state<FieldError | null>(null);
  const errorFor = (fid: string): string => (fieldErr?.id === fid ? fieldErr.message : '');
  /** The Save bar's line: anything that is not about one field. */
  const formError = $derived(error || (fieldErr?.id === null ? fieldErr.message : ''));
  /** "More details": opened for an entry that already uses any of its fields. */
  let moreOpen = $state(false);
  /** True until the object (and, when editing, the entry) has been applied to the form
   *  (`aria-busy`): "More details" may still open itself until then. */
  let loading = $state(true);
  let allSuggestions = $state<TitleSuggestion[]>([]);
  /** Tags already in use, offered while typing one. */
  let tagCounts = $state<TagCount[]>([]);
  const suggestions = $derived(suggestionsFor(allSuggestions, input.category));
  // The object's vocabulary, plus whatever this entry already says. An entry logged before its
  // object was re-typed must keep its own category in the list, or saving an untouched form
  // would quietly re-file it.
  const resourceUnit = $derived(object?.resource_unit ?? object?.fuel_unit ?? null);
  const offered = $derived(categoriesFor(object?.type ?? 'other', $customTypes, input.category, object?.counter_unit, resourceUnit, object?.resource_kind));
  /** Set once the user picks a category (the select, or a repeat chip). Until then a new entry's
   *  category is only a default, and may be re-chosen when the object's own type loads late. */
  let categoryTouched = false;
  /** True when `saved` exists only because the user attached a file, never because they saved. */
  let autoDraft = $state(false);
  /** False only while editing an existing activity whose GET hasn't resolved yet — blocks the
   *  "add files" affordance so it can't spuriously POST a new row before we know one already exists. */
  let ready = $state(untrack(() => aid === undefined));

  const lastCounter = $derived(object?.stats.current_counter ?? null);
  const counterWarn = $derived(counterBelowLast(counterText, lastCounter));
  const weightWarn = $derived(weightDeviates(input.category, weightText, weightUnit, object?.stats.latest_weight_grams));
  const photoDate = $derived(firstExifDate(attachments));
  const previousWeight = $derived(object?.stats.latest_weight_grams != null
    ? `${$t('weight.previous')}: ${formatWeight(object.stats.latest_weight_grams, weightUnit, $locale)} · ${fmtDate(object.stats.latest_weight_date ?? null, $dateFormat)}`
    : '');

  /** The `?category=` query parameter of a `.../activities/new` link -- ObjectDetail's "+ Log
   *  trip" button uses it (see Step 4 of the trip-log task). A garbage or unknown value is
   *  simply ignored, same as `offered`'s own fallback below would ignore a category the object's
   *  type does not actually offer. */
  function categoryParam(): Category | null {
    return parseCategoryParam(location.search);
  }

  onMount(async () => {
    try { await load(); } finally { loading = false; }
  });

  async function load() {
    // Not awaited, and a failure is ignored: this form has to work offline, and suggestions are
    // only a convenience. `tags` itself travels in `input`, so the outbox carries it like `notes`.
    api<TagCount[]>('GET', '/tags').then((list) => (tagCounts = list), () => {});
    try {
      object = await api<MemObject>('GET', `/objects/${oid}`);
      setCachedObject(oid, object);
    } catch (e) {
      // Offline (or a dead/slow connection): fall back to the last object this session saw,
      // the same way ObjectDetail's loadObject() does -- without this, `object` stays null and
      // the odometer / fuel-quantity fields below (gated on `object?.counter_unit`) silently
      // vanish, even though this form exists precisely to log things like a garage fill-up
      // while offline. Never on a genuine rejection (`isRejection`): a 401/403/404 is the
      // server answering, possibly about an object that belongs to someone else entirely.
      const cached = isRejection(e) ? undefined : getCachedObject(oid);
      if (cached) object = cached;
      else error = errorMessage(e, $t);
    }
    // A new entry's default category (`emptyActivity`'s 'maintenance') isn't offered by every
    // type -- a `body` object offers no `maintenance` at all -- so the select would silently
    // sit on an option that isn't in its own list. Editing overwrites `input` wholesale below,
    // so this only ever matters for a genuinely new entry. A `?category=` link (ObjectDetail's
    // "+ Log trip") wins over that default, but ONLY when the object actually offers it --
    // `offered` below re-derives the very same list reactively for the select itself, so the
    // two can never disagree about what a stale/forged query value is allowed to pick.
    if (!aid && object) {
      const unit = object.resource_unit ?? object.fuel_unit ?? null;
      const list = categoriesFor(object.type, $customTypes, undefined, object.counter_unit, unit, object.resource_kind);
      const picked = resolveCategory(list, categoryParam(), input.category);
      input.category = picked.category;
      if (picked.touched) categoryTouched = true;
    }
    try {
      allSuggestions = await api<TitleSuggestion[]>('GET', `/objects/${oid}/recent-titles`);
    } catch {
      // Repeat chips are a convenience, and this form exists to be usable in a garage with no
      // signal. Letting this reject would abandon the whole of onMount below it -- including,
      // when editing, the load of the very row being edited.
    }
    weightUnit = object?.weight_unit ?? 'kg';
    if (aid) {
      try {
        const a = await api<Activity>('GET', `/activities/${aid}`);
        saved = a;
        autoDraft = false; // this row predates the form; never let a stray click earlier mark it disposable
        input = toActivityInput(a);
        const f = activityToFormText(a, weightUnit);
        weightText = f.weightText; costText = f.costText; counterText = f.counterText; quantityText = f.quantityText;
        meterReadingText = f.meterReadingText; chargedFull = f.chargedFull; fromText = f.fromText; toText = f.toText;
        durationText = f.durationText; distance = f.distance;
        originalWeightText = weightText; originalWeightUnit = weightUnit; originalWeight = a.weight_grams ?? null;
        attachments = a.attachments;
        moreOpen = activityHasDetails(input);
        ready = true;
      } catch (e) {
        // The row could not be loaded, so the form is showing empty defaults on an EDIT url.
        // Say so: `submit` refuses to save in this state, and silently rendering a blank form
        // is what let an offline edit become a brand-new second activity.
        error = errorMessage(e, $t);
      }
    } else if (object && object.stats.current_counter !== null) {
      counterText = String(object.stats.current_counter);
    }
  }

  // Re-check whenever the object context arrives. Svelte derived values update on the next
  // reactive pass, so reading `resourceUnit` immediately after assigning `object` in onMount can
  // still see null. This also handles own types that load after the form itself.
  $effect(() => {
    const list = $customTypes;
    if (aid || !object || categoryTouched) return;
    const unit = object.resource_unit ?? object.fuel_unit ?? null;
    const own = categoriesFor(object.type, list, undefined, object.counter_unit, unit, object.resource_kind);
    const picked = resolveCategory(own, categoryParam(), untrack(() => input.category));
    input.category = picked.category;
    if (picked.touched) categoryTouched = true;
  });

  // A new trip's start defaults to the object's current counter -- "from where the odometer
  // already is" -- the moment the category is (or becomes) trip, whether that happened via the
  // `?category=trip` query above or a manual pick in the select further down. Guarded to run
  // once: without `tripStartInit`, re-entering an empty Start field after clearing it would keep
  // snapping back to the object's counter on every unrelated re-render.
  let tripStartInit = false;
  $effect(() => {
    if (aid || tripStartInit || input.category !== 'trip') return;
    if (!object || object.stats.current_counter === null) return;
    tripStartInit = true;
    if (input.start_counter === null) input.start_counter = object.stats.current_counter;
  });

  // From/To suggest this object's own earlier trip places (see `TripPlaces`). Loaded once, the
  // moment the category is (or becomes) trip -- not in `onMount` unconditionally, since most
  // entries are never a trip and the object may not even offer one yet when this form opens.
  // Failure is ignored: the datalist is a convenience, not something this form depends on.
  let tripPlacesLoaded = false;
  $effect(() => {
    if (tripPlacesLoaded || input.category !== 'trip') return;
    tripPlacesLoaded = true;
    api<TripPlaces>('GET', `/objects/${oid}/trip-places`).then((p) => (tripPlaces = p), () => {});
  });

  /** The three linked fields as `linkTrip*` (../lib/trip.ts) takes and returns them.
   *  `start_counter` is declared optional on `ActivityInput` (`?:`, for the benefit of every
   *  non-trip caller that never sets it at all) -- `?? null` reads that absent case the same as
   *  an explicit null, since the two mean the same thing here: no start typed yet. */
  const tripLink = (): TripLink => ({ start: input.start_counter ?? null, end: input.counter_value, distance });

  /** End typed directly: distance follows it, same as the spec says -- the actual arithmetic
   *  (including clearing distance when End is cleared) lives in `linkTripEnd`, tested on its
   *  own in `tests/trip.test.ts`. */
  function onTripEndChange() {
    distance = linkTripEnd(tripLink()).distance;
  }
  /** Distance typed directly: end follows it (start + distance), unless start is not known yet. */
  function onTripDistanceChange() {
    input.counter_value = linkTripDistance(tripLink()).end;
  }
  /** Start changed: an already-known distance is kept and the end moves with it; with no
   *  distance yet (a freshly prefilled or freshly typed start, end not yet touched) there is
   *  nothing to move, so this falls back to deriving distance from whatever end is already there. */
  function onTripStartChange() {
    const linked = linkTripStart(tripLink());
    input.counter_value = linked.end;
    distance = linked.distance;
  }

  // `saved.id` is the temp id ActivityForm minted for its own draft (see `mintTempId` below)
  // for as long as `saved.pending` holds. Nothing else refreshes it once a BACKGROUND flush --
  // not this form's own await, e.g. the ordinary mobile path where returning from the camera
  // fires `visibilitychange`, which triggers a flush -- lands the create: without this, `saved`
  // keeps naming an id no row will ever have, so a second attached photo queues against a
  // temp id whose create is already gone (never resolves, 404s, dies), Save takes the
  // `pending` branch against a queued op that no longer exists (silently discarding whatever
  // was typed), and Cancel finds nothing to remove. Rewriting `saved` here -- which flows
  // straight through to `FilePicker`'s `activityId` prop, since that reads `saved.id` -- is
  // what makes the resolution visible to every one of those call sites at once.
  $effect(() => onOutboxFlushed((resolved) => {
    if (saved?.pending && resolved.has(saved.id)) {
      saved = { ...saved, id: resolved.get(saved.id)!, pending: false };
    }
  }));

  /** A negative id for the draft being created underground, so a file upload has a parent id
   *  to attach to before the server has assigned a real one. Negative so it can never collide
   *  with a real (always positive) activity id -- `hashToNegativeId` in ../lib/activity-form.ts,
   *  which ObjectDetail also uses, though there it hashes an existing op id rather than minting
   *  a fresh one -- this id is what gets passed to `createQueued` as `tempId`, so it is also the exact
   *  id the outbox stores and later rewrites (see `persistResolvedId` in ../lib/outbox.ts). */
  function mintTempId(): number {
    return hashToNegativeId(newOpId()); // not crypto.randomUUID: absent on a plain-http origin (see ../lib/outbox.ts)
  }

  /** Files need an activity row to hang on, so save the draft first.
   *
   *  Wrapped in `serialize` (../lib/outbox.ts) so two fast taps on "+ Add files" -- both
   *  reading `saved === null` before either await settles -- share one in-flight save instead
   *  of each minting its own temp id and queuing its own `activity.create`; offline that would
   *  otherwise leave two drafts behind for what the user experienced as one tap. */
  const ensureSaved = serialize(async (): Promise<Activity> => {
    if (saved) return saved;
    if (!ready) throw new Error('activity.not-loaded');
    let body = buildInput();
    let bad = validateActivity(body);
    if (bad === 'activity.title') {
      // Snapping the receipt comes before naming the entry, and the draft needs a title to be
      // saved at all. The category is a fair one to start with -- "Repair" beside a photo of
      // the repair bill -- and it sits in the title field in plain sight, to be replaced.
      input.title = $t(`cat.${input.category}`);
      body = buildInput();
      bad = validateActivity(body);
    }
    if (bad) throw new Error(fieldError(bad, $t));
    const tempId = mintTempId();
    // `createQueued` returns null when the write only reached the outbox (offline). Passing
    // the SAME tempId as its `tempId` argument means the outbox stores it on the queued op, so
    // an upload queued below against this id gets rewritten to the real one once the create
    // lands (see `persistResolvedId` in ../lib/outbox.ts) -- not a second, unrelated numbering
    // scheme invented here.
    const result = await createQueued<Activity>(`/objects/${oid}/activities`, body as unknown as Record<string, unknown>, tempId);
    saved = result ?? optimisticActivity(tempId, oid, body);
    autoDraft = true;
    return saved;
  });

  function changeWeightUnit(next: WeightUnit) {
    const s = changeWeightUnitState({ text: weightText, unit: weightUnit, originalText: originalWeightText, originalUnit: originalWeightUnit, original: originalWeight }, next);
    weightText = s.text; weightUnit = s.unit; originalWeightText = s.originalText; originalWeightUnit = s.originalUnit; originalWeight = s.original;
  }

  function buildInput(): ActivityInput {
    return buildActivityInput(
      input,
      { costText, counterText, quantityText, meterReadingText, fromText, toText, durationText, chargedFull },
      { text: weightText, unit: weightUnit, originalText: originalWeightText, originalUnit: originalWeightUnit, original: originalWeight },
      object, $t,
    );
  }

  /** Prefill from a past entry. The user still reviews and saves; nothing is written here. */
  function repeat(s: TitleSuggestion) {
    input.title = s.title;
    input.category = s.category;
    categoryTouched = true;
    if (s.last_cost_cents !== null) costText = centsToInput(s.last_cost_cents);
    // A trip suggestion also carries where it went, so repeating one prefills From/To exactly
    // as it already prefills title/category/cost -- `last_from_place`/`last_to_place` are only
    // ever set on a trip suggestion in the first place (see `TitleSuggestion` on the backend).
    if (s.last_from_place !== null) fromText = s.last_from_place;
    if (s.last_to_place !== null) toText = s.last_to_place;
    if (s.last_from_place !== null || s.last_to_place !== null) moreOpen = true;
  }

  async function reject(key: string) {
    const at = fieldErrorAt(key, $t, ACTIVITY_FIELD_IDS);
    fieldErr = at;
    if (at.id !== null && !(await revealField(at.id))) fieldErr = { id: null, message: at.message };
  }

  /** The first "+ Add photos or files": saves the draft the files will hang on (`ensureSaved`). */
  async function addFiles() {
    try { await ensureSaved(); error = ''; } catch (e) { error = errorMessage(e, $t); }
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    fieldErr = null;
    if (editing && !saved) {
      // On an edit url with no loaded row: `onMount`'s GET failed (offline, a 5xx). Falling
      // through would take the `else` branch below and CREATE a second activity -- the user
      // asked to change one entry and would silently get two, with their edit on the copy.
      error = $t('activity.not-loaded');
      return;
    }
    const body = buildInput();
    const bad = validateActivity(body);
    if (bad) { await reject(bad); return; }
    busy = true; error = '';
    try {
      if (saved?.pending) {
        // The draft's create is still only queued -- there is no server row to PATCH, and the
        // outbox has no "edit" op kind (see OpKind in ../lib/outbox.ts). Fold whatever changed
        // since "+ Add files" queued it into that same op, instead of attempting a PATCH that
        // would just fail offline and strand the user on this form.
        //
        // `saved.id` can go stale: a background flush (see the `onOutboxFlushed` subscription
        // above) may have already landed this exact create and rewritten `saved`, in the
        // ordinary case, before this ever runs -- but a fold that still lands on no live op
        // (a race the subscription didn't win) must not be treated as success. Silently doing
        // nothing here and navigating away regardless is exactly how everything typed after
        // "+ Add files" used to get discarded with no error at all.
        const folded = await updateQueuedActivity(saved.id, body as unknown as Record<string, unknown>);
        if (!folded) throw new Error('activity.save-lost');
      } else if (saved) {
        // Queued with the moment of the edit when the connection is gone; the server keeps each
        // field only if nothing newer changed it meanwhile (see `updateQueued` in ../lib/api.ts).
        await updateQueued(`/activities/${saved.id}`, body as unknown as Record<string, unknown>);
      } else {
        // null means "queued, not sent": the row exists locally and will be replayed.
        await createQueued<Activity>(`/objects/${oid}/activities`, body as unknown as Record<string, unknown>);
      }
      autoDraft = false;
      go(`/objects/${oid}`, true);
    } catch (err) {
      // `createQueued` throws an i18n key (rather than a message) when the write reached
      // neither the server nor the local outbox queue, so the entry is honestly reported as
      // lost instead of navigating away as though it had been saved. `errorMessage` translates
      // that key and says any other failure in the reader's language too.
      error = errorMessage(err, $t);
    } finally { busy = false; }
  }

  /** Cancel throws the auto-created draft away; keeping it would leave a stray timeline entry. */
  async function cancel() {
    if (autoDraft && saved) {
      if (attachments.length > 0 && !confirm($t('activity.discard-draft'))) return;
      if (saved.pending) {
        // Only the outbox has this draft -- there is no server row to DELETE (that would
        // 404), and leaving the queued create (or an upload still naming its temp id) behind
        // would replay it later, creating exactly the stray entry this cancel exists to avoid.
        // A background pass may have landed the draft since, without this form hearing yet: the
        // row is then DELETEd, and a failure to is shown, exactly as in the branch below.
        try {
          await cancelQueuedActivity(saved.id);
        } catch (e) {
          error = errorMessage(e, $t);
          return;
        }
      } else {
        // The create already reached the server (`saved.id` is a real id), so the row exists
        // and must be DELETEd -- but the network can still have died since, e.g. between the
        // save and attaching another photo, which would then have queued against this REAL
        // id. Silently swallowing a failed DELETE used to navigate away as though the row were
        // gone while it (and now a queued upload naming it) both survived; a queued upload for
        // it would then replay later and land on an activity the user was told was cancelled.
        // So: only drop the queued upload -- and only leave the form -- once the DELETE is
        // actually confirmed, and surface a real error otherwise instead of pretending success.
        try {
          await api('DELETE', `/activities/${saved.id}`);
        } catch (e) {
          error = errorMessage(e, $t);
          return;
        }
        try { await cancelQueuedActivity(saved.id); } catch { /* best-effort cleanup; the row is already gone server-side */ }
      }
    }
    back(`/objects/${oid}`);
  }

  async function remove() {
    if (!saved || !confirm($t('nav.confirm-delete'))) return;
    // Sibling of the same fix already made in cancel(): a failed DELETE used to throw with
    // nothing surfaced, silently stranding the user with no idea the delete never happened.
    try {
      await api('DELETE', `/activities/${saved.id}`);
    } catch (e) {
      error = errorMessage(e, $t);
      return;
    }
    go(`/objects/${oid}`, true);
  }
</script>

<main>
  <TopBar title={editing ? $t('activity.edit') : input.category === 'weight' ? $t('weight.log') : $t('activity.new')} backTo={`/objects/${oid}`} />
  <form onsubmit={submit} aria-busy={loading} class="m-0 flex w-full max-w-[40rem] flex-col gap-5">
    <div class="grid grid-cols-2 gap-3">
      <Field id="d" label={$t('activity.date')} error={errorFor('d')}>
        <DateInput id="d" bind:value={input.date} max={input.category === 'weight' ? todayIso() : undefined} required />
      </Field>
      <Field id="c" label={$t('activity.category')}>
        <NativeSelect bind:value={() => input.category, (v: Category) => { input.category = v; categoryTouched = true; }}>
          {#each offered as c (c)}<option value={c}>{$t(`cat.${c}`)}</option>{/each}
        </NativeSelect>
      </Field>
    </div>
    {#if photoDate && photoDate !== input.date}
      <button type="button" data-slot="photo-date" onclick={() => (input.date = photoDate)}
              class="-mt-3 min-h-11 w-fit cursor-pointer text-left text-sm font-medium text-brand-ink underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">
        {$t('activity.use-exif-date', { date: fmtDate(photoDate, $dateFormat) })}
      </button>
    {/if}
    {#if input.category !== 'weight' && !editing && suggestions.length > 0}
      <div class="-mx-1 -mt-2 flex gap-2 overflow-x-auto px-1 py-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {#each suggestions.slice(0, 3) as s (s.title + s.category)}
          <button type="button" data-slot="repeat-chip" class={chipClass} onclick={() => repeat(s)}>{$t('activity.repeat')}: {s.title}</button>
        {/each}
      </div>
    {/if}

    {#if input.category === 'weight'}
      <div class="grid grid-cols-[1fr_7rem] gap-3">
        <Field id="weight" label={$t('cat.weight')} hint={previousWeight} warn={weightWarn ? $t('weight.large-change') : ''} error={errorFor('weight')}>
          <Input type="text" inputmode="decimal" bind:value={weightText} required />
        </Field>
        <Field id="weight-unit" label={$t('weight.unit')}>
          <NativeSelect bind:value={() => weightUnit, changeWeightUnit}><option value="kg">kg</option><option value="lb">lb</option></NativeSelect>
        </Field>
      </div>
    {:else}
      <!-- Optional only for a trip, a charge or a usage: the fallback word is the placeholder,
           not pre-filled text to notice and delete. `activityTitle` is what the timeline shows
           for an untitled row, so the two never disagree. -->
      <Field id="ti" label={$t('activity.title')} error={errorFor('ti')}>
        <Input list="titles" bind:value={input.title} required={input.category !== 'trip' && input.category !== 'fuel' && input.category !== 'usage'}
               placeholder={activityTitle('', input.category, $t, object?.fuel_unit ?? undefined) || undefined} />
        <datalist id="titles">{#each suggestions as s (s.title + s.category)}<option value={s.title}></option>{/each}</datalist>
      </Field>
    {/if}

    {#if input.category === 'usage' && object?.resource_kind === 'water' && object.measurement_mode === 'meter'}
      <Field id="meter-reading" label={$t('water.meter-reading')} unit={fuelUnitLabel(resourceUnit)} error={errorFor('meter-reading')}>
        <Input type="text" inputmode="decimal" bind:value={meterReadingText} required />
      </Field>
    {/if}
    {#if input.category === 'fuel' || (input.category === 'usage' && object?.resource_kind !== 'water')}
      <!-- "Charged full" for a kWh object, "Filled up" for a tank, picked like every other
           charge/fill string (energyLabelKey). -->
      <CheckField id="charged-full" label={$t(energyLabelKey(resourceUnit) === 'energy.charged' ? 'activity.charged-full' : 'activity.filled-full')} bind:checked={chargedFull} />
    {/if}

    {#if input.category === 'trip'}
      <!-- `object?.counter_unit`: an existing trip must stay editable offline even when the
           object could not be loaded; the fields then show no unit. -->
      <div class="grid grid-cols-2 gap-3">
        <Field id="tst" label={$t('trip.start')} unit={object?.counter_unit ?? null} error={errorFor('tst')}>
          <Input type="number" inputmode="numeric" min="0" bind:value={() => input.start_counter, (v) => { input.start_counter = v; onTripStartChange(); }} />
        </Field>
        <Field id="ten" label={$t('trip.end')} unit={object?.counter_unit ?? null} error={errorFor('ten')}>
          <Input type="number" inputmode="numeric" min="0" bind:value={() => input.counter_value, (v) => { input.counter_value = v; onTripEndChange(); }} />
        </Field>
      </div>
      <!-- No `min` on Distance: an end below start makes it negative, which is exactly what
           `trip.error-end` explains; a native bound would block the submit before it could. -->
      <Field id="tds" label={$t('trip.distance')} unit={object?.counter_unit ?? null}>
        <Input type="number" inputmode="numeric" bind:value={() => distance, (v) => { distance = v; onTripDistanceChange(); }} />
      </Field>
    {/if}
    {#if input.category === 'session'}
      <div class="grid grid-cols-2 gap-3">
        <Field id="session-location" label={$t('session.location')}><Input maxlength={80} bind:value={fromText} /></Field>
        <Field id="session-duration" label={$t('session.duration')}><Input type="text" inputmode="numeric" placeholder="h:mm" bind:value={durationText} /></Field>
      </div>
    {/if}

    {#if input.category !== 'weight'}
      {@const withCounter = !!object?.counter_unit && input.category !== 'trip'}
      <div class="grid grid-cols-2 gap-3">
        {#if withCounter && object?.counter_unit}
          <Field id="cv" label={$t('activity.counter')} unit={object.counter_unit} error={errorFor('cv')}
                 warn={counterWarn ? $t('activity.counter-warn', { last: fmtCounter(lastCounter, object.counter_unit, $locale) }) : ''}>
            <Input type="number" inputmode="numeric" min="0" bind:value={counterText} />
          </Field>
        {/if}
        <Field id="co" label={$t('activity.cost')} error={errorFor('co')} class={withCounter ? '' : 'col-span-2'}>
          <Input type="text" inputmode="decimal" bind:value={costText} />
        </Field>
      </div>
    {/if}
    {#if (input.category === 'fuel' || (input.category === 'usage' && object?.measurement_mode !== 'meter')) && resourceUnit}
      <Field id="qt" label={$t('activity.quantity')} unit={fuelUnitLabel(resourceUnit)} error={errorFor('qt')}>
        <Input type="text" inputmode="decimal" bind:value={quantityText} />
      </Field>
    {/if}

    <section aria-labelledby="activity-photos" class="flex flex-col gap-3">
      <h2 id="activity-photos" class={sectionHeadingClass}>{$t('activity.photos')}</h2>
      {#if attachments.length > 0}
        <ul data-testid="entry-attachments" role="list" class="m-0 flex list-none gap-2 overflow-x-auto p-0">
          {#each attachments as a (a.id)}
            <!-- The item is the image's own size: a positioning context for the "pending" label.
                 Only the thumbnail fades while queued; the label stays solid (muted-foreground on
                 muted, the tested pair). -->
            <li data-testid="attachment" data-pending={a.pending ? '' : undefined} class="group relative shrink-0">
              {#if a.kind === 'photo'}
                <img class="block size-16 rounded-md object-cover group-data-pending:opacity-60" src={a.pending ? a.previewUrl : fileUrl(a.file_id, true)} alt="" loading="lazy" decoding="async" />
              {:else}
                <span class="grid size-16 place-items-center rounded-md bg-muted text-muted-foreground group-data-pending:opacity-60"><Icon name="document" size={28} /></span>
              {/if}
              {#if a.pending}<span class="absolute inset-x-0.5 bottom-0.5 rounded-sm bg-muted px-0.5 text-center text-xs leading-tight text-muted-foreground">{$t('timeline.pending')}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      {#if saved}
        <FilePicker objectId={oid} activityId={saved.id} onuploaded={(a) => (attachments = [...attachments, a])} />
      {:else if ready}
        <Button variant="outline" class="min-h-11 w-full border-dashed" onclick={addFiles}>+ {$t('activity.add-files')}</Button>
      {/if}
    </section>

    <MoreDetails bind:open={moreOpen}>
      <Field id="no" label={$t('activity.notes')}><Textarea bind:value={input.notes} /></Field>
      <TagInput bind:tags={() => input.tags ?? [], (v) => (input.tags = v)} suggestions={tagCounts} label={$t('tags.label')} id="tags" />
      {#if input.category === 'trip'}
        <div class="grid grid-cols-2 gap-3">
          <Field id="tfr" label={$t('trip.from')}>
            <Input list="trip-from" maxlength={80} bind:value={fromText} />
            <datalist id="trip-from">{#each tripPlaces.from as p (p)}<option value={p}></option>{/each}</datalist>
          </Field>
          <Field id="tto" label={$t('trip.to')}>
            <Input list="trip-to" maxlength={80} bind:value={toText} />
            <datalist id="trip-to">{#each tripPlaces.to as p (p)}<option value={p}></option>{/each}</datalist>
          </Field>
        </div>
        <div class="grid grid-cols-2 gap-3">
          <Field id="tdu" label={$t('trip.duration')} error={errorFor('tdu')}>
            <Input type="text" inputmode="numeric" placeholder="h:mm" bind:value={durationText} />
          </Field>
          <!-- No `min`/`max`: 101 must reach `trip.error-battery` instead of a silent refusal. -->
          <Field id="tba" label={$t('trip.battery')} unit="%" error={errorFor('tba')}>
            <Input type="number" inputmode="numeric" bind:value={input.battery_used_pct} />
          </Field>
        </div>
      {/if}
      {#if (input.category === 'fuel' || (input.category === 'usage' && object?.resource_kind !== 'water')) && (resourceUnit === 'l' || resourceUnit === 'gal')}
        <Field id="fuel-level" label={$t('activity.fuel-level')} hint={$t('activity.fuel-level-hint')} error={errorFor('fuel-level')}>
          <Input type="number" inputmode="numeric" min="0" max="100" bind:value={input.fuel_level_pct} />
        </Field>
      {/if}
      {#if input.category === 'usage' && object?.resource_kind === 'water'}
        {#if object.measurement_mode === 'meter'}
          <CheckField id="meter-reset" label={$t('water.meter-reset')} bind:checked={() => input.meter_reset === 1, (v) => (input.meter_reset = v ? 1 : 0)} />
        {:else}
          <div class="grid grid-cols-2 gap-3">
            <Field id="period-start" label={$t('water.period-start')} error={errorFor('period-start')}>
              <DateInput id="period-start" bind:value={() => input.period_start ?? '', (v) => (input.period_start = v || null)} />
            </Field>
            <Field id="period-end" label={$t('water.period-end')}>
              <DateInput id="period-end" bind:value={() => input.period_end ?? '', (v) => (input.period_end = v || null)} />
            </Field>
          </div>
        {/if}
        <CheckField id="estimated" label={$t('water.estimated')} bind:checked={() => input.estimated === 1, (v) => (input.estimated = v ? 1 : 0)} />
      {/if}
    </MoreDetails>

    <FormActions {busy} error={formError} oncancel={cancel} />
  </form>
  {#if editing}
    <section aria-labelledby="activity-delete" class="mt-8 flex max-w-[40rem] flex-col gap-2 border-t border-border pt-4">
      <h2 id="activity-delete" class={sectionHeadingClass}>{$t('nav.delete')}</h2>
      <Button variant="destructive" class="min-h-11 w-fit" onclick={remove}>{$t('nav.delete')}</Button>
    </section>
  {/if}
</main>
