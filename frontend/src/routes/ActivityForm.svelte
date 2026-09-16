<script lang="ts">
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
  import { centsToInput, counter as fmtCounter, fmtDate, parseMoney, parseQuantity } from '../lib/format';
  import { dateFormat } from '../stores/date-format';
  import { activityTitle, emptyActivity, exifDate, suggestionsFor, toActivityInput, validateActivity } from '../lib/activity-form';
  import { fieldError } from '../lib/form-error';
  import { formatDuration, linkTripDistance, linkTripEnd, linkTripStart, parseDuration, tripDistance, type TripLink } from '../lib/trip';
  import { fuelUnitLabel } from '../lib/energy';
  import { categoriesFor, customTypes } from '../lib/type-registry';
  import { locale, t } from '../i18n';
  import { CATEGORIES, type Activity, type Attachment, type Category, type MemObject, type ActivityInput, type TagCount, type TitleSuggestion, type TripPlaces } from '../lib/types';

  let { id, aid }: { id: string; aid?: string } = $props();
  const oid = $derived(Number(id));
  const editing = $derived(aid !== undefined);

  let object = $state<MemObject | null>(null);
  let input = $state<ActivityInput>(emptyActivity());
  let costText = $state('');
  let counterText = $state('');
  let quantityText = $state('');
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
  let allSuggestions = $state<TitleSuggestion[]>([]);
  /** Tags already in use, offered while typing one. */
  let tagCounts = $state<TagCount[]>([]);
  const suggestions = $derived(suggestionsFor(allSuggestions, input.category));
  // The object's vocabulary, plus whatever this entry already says. An entry logged before its
  // object was re-typed must keep its own category in the list, or saving an untouched form
  // would quietly re-file it.
  const offered = $derived(categoriesFor(object?.type ?? 'other', $customTypes, input.category, object?.counter_unit));
  /** Set once the user picks a category (the select, or a repeat chip). Until then a new entry's
   *  category is only a default, and may be re-chosen when the object's own type loads late. */
  let categoryTouched = false;
  /** True when `saved` exists only because the user attached a file, never because they saved. */
  let autoDraft = $state(false);
  /** False only while editing an existing activity whose GET hasn't resolved yet — blocks the
   *  "add files" affordance so it can't spuriously POST a new row before we know one already exists. */
  let ready = $state(untrack(() => aid === undefined));

  const lastCounter = $derived(object?.stats.current_counter ?? null);
  const counterWarn = $derived(
    counterText !== '' && lastCounter !== null && Number(counterText) < lastCounter,
  );
  const photoDate = $derived(attachments.map(exifDate).find((d) => d !== null) ?? null);

  /** The `?category=` query parameter of a `.../activities/new` link -- ObjectDetail's "+ Log
   *  trip" button uses it (see Step 4 of the trip-log task). A garbage or unknown value is
   *  simply ignored, same as `offered`'s own fallback below would ignore a category the object's
   *  type does not actually offer. */
  function categoryParam(): Category | null {
    const c = new URLSearchParams(location.search).get('category');
    return c && (CATEGORIES as readonly string[]).includes(c) ? (c as Category) : null;
  }

  onMount(async () => {
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
      else error = (e as Error).message;
    }
    // A new entry's default category (`emptyActivity`'s 'maintenance') isn't offered by every
    // type -- a `body` object offers no `maintenance` at all -- so the select would silently
    // sit on an option that isn't in its own list. Editing overwrites `input` wholesale below,
    // so this only ever matters for a genuinely new entry. A `?category=` link (ObjectDetail's
    // "+ Log trip") wins over that default, but ONLY when the object actually offers it --
    // `offered` below re-derives the very same list reactively for the select itself, so the
    // two can never disagree about what a stale/forged query value is allowed to pick.
    if (!aid && object) {
      const list = categoriesFor(object.type, $customTypes, undefined, object.counter_unit);
      const wanted = categoryParam();
      if (wanted && list.includes(wanted)) {
        input.category = wanted;
        categoryTouched = true;
      } else if (!list.includes(input.category)) {
        input.category = list[0];
      }
    }
    try {
      allSuggestions = await api<TitleSuggestion[]>('GET', `/objects/${oid}/recent-titles`);
    } catch {
      // Repeat chips are a convenience, and this form exists to be usable in a garage with no
      // signal. Letting this reject would abandon the whole of onMount below it -- including,
      // when editing, the load of the very row being edited.
    }
    if (aid) {
      try {
        const a = await api<Activity>('GET', `/activities/${aid}`);
        saved = a;
        autoDraft = false; // this row predates the form; never let a stray click earlier mark it disposable
        input = toActivityInput(a);
        costText = centsToInput(a.cost_cents);
        counterText = a.counter_value === null ? '' : String(a.counter_value);
        quantityText = a.quantity_milli === null ? '' : String(a.quantity_milli / 1000);
        chargedFull = a.charged_full === 1;
        fromText = a.from_place ?? '';
        toText = a.to_place ?? '';
        durationText = a.duration_minutes === null ? '' : formatDuration(a.duration_minutes);
        distance = tripDistance(a);
        attachments = a.attachments;
        ready = true;
      } catch (e) {
        // The row could not be loaded, so the form is showing empty defaults on an EDIT url.
        // Say so: `submit` refuses to save in this state, and silently rendering a blank form
        // is what let an offline edit become a brand-new second activity.
        error = $t((e as Error).message);
      }
    } else if (object && object.stats.current_counter !== null) {
      counterText = String(object.stats.current_counter);
    }
  });

  // On a cold load the own types can arrive after `onMount` picked the default above: until then
  // an own type offers every category, so `maintenance` looked fine. Re-check when they land, as
  // long as the entry is new and its category untouched. Built-in types never depend on the
  // list, so they keep exactly the `onMount` behaviour.
  $effect(() => {
    const list = $customTypes;
    if (aid || !object || categoryTouched || !object.type.startsWith('custom:')) return;
    const own = categoriesFor(object.type, list, undefined, object.counter_unit);
    if (!own.includes(untrack(() => input.category))) input.category = own[0];
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
   *  with a real (always positive) activity id -- same scheme as `pendingId` in
   *  ObjectDetail.svelte, though that one hashes an existing op id rather than minting a fresh
   *  one -- this id is what gets passed to `createQueued` as `tempId`, so it is also the exact
   *  id the outbox stores and later rewrites (see `persistResolvedId` in ../lib/outbox.ts). */
  function mintTempId(): number {
    const s = newOpId(); // not crypto.randomUUID: absent on a plain-http origin (see ../lib/outbox.ts)
    let h = 0;
    for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
    return -(Math.abs(h) || 1);
  }

  /** Files need an activity row to hang on, so save the draft first.
   *
   *  Wrapped in `serialize` (../lib/outbox.ts) so two fast taps on "+ Add files" -- both
   *  reading `saved === null` before either await settles -- share one in-flight save instead
   *  of each minting its own temp id and queuing its own `activity.create`; offline that would
   *  otherwise leave two drafts behind for what the user experienced as one tap. */
  const ensureSaved = serialize(async (): Promise<Activity> => {
    if (saved) return saved;
    if (!ready) throw new Error('not loaded yet');
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
    saved = result ?? {
      id: tempId, object_id: oid, ...body, tags: body.tags ?? [],
      start_counter: body.start_counter ?? null, from_place: body.from_place ?? null,
      to_place: body.to_place ?? null, duration_minutes: body.duration_minutes ?? null,
      battery_used_pct: body.battery_used_pct ?? null, charged_full: body.charged_full ?? 0,
      created_at: new Date().toISOString(), updated_at: new Date().toISOString(),
      attachments: [], pending: true,
    };
    autoDraft = true;
    return saved;
  });

  function buildInput(): ActivityInput {
    const isTrip = input.category === 'trip';
    return {
      ...input,
      // A plain copy: `input.tags` is a $state proxy, and IndexedDB cannot clone a proxy, so
      // queuing this body offline (the outbox) would fail with the spread's array left as it is.
      tags: [...(input.tags ?? [])],
      cost_cents: parseMoney(costText),
      // A trip's end IS the counter (`input.counter_value` is bound straight to the End field
      // below, unlike the generic Counter field's own `counterText`); every other category keeps
      // reading the generic field, exactly as before.
      counter_value: isTrip ? input.counter_value : (String(counterText).trim() === '' ? null : Number(counterText)),
      // The quantity field only exists in the form for the fuel category (see the template
      // below) -- send it only then, so switching category away from fuel after typing an
      // amount can't leave a fuel quantity stuck on a repair/maintenance/... row.
      // Same comma/dot handling as parseMoney, so this field and cost agree on what's valid input.
      quantity_milli: input.category === 'fuel' ? parseQuantity(quantityText) : null,
      // Same pattern as `quantity_milli` just above: 1 only while this IS a fuel entry and the
      // box is ticked, 0 otherwise -- so switching category away from fuel after ticking it
      // can't leave the flag stuck set on a repair/maintenance/... row (the backend rejects it
      // there outright: "only a charge can be marked full").
      charged_full: input.category === 'fuel' && chargedFull ? 1 : 0,
      // The five trip fields exist in the form only for the trip category (see the template
      // below) -- sent as null otherwise, mirroring `quantity_milli` above, so switching away
      // from trip after filling any of them in can't leave them stuck on a repair/maintenance/...
      // row (the backend rejects them there outright, and PATCH keeps whatever it last stored
      // when a field is merely absent from the body).
      start_counter: isTrip ? input.start_counter : null,
      from_place: isTrip ? (fromText.trim() || null) : null,
      to_place: isTrip ? (toText.trim() || null) : null,
      duration_minutes: isTrip ? parseDuration(durationText) : null,
      battery_used_pct: isTrip ? input.battery_used_pct : null,
    };
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
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (editing && !saved) {
      // On an edit url with no loaded row: `onMount`'s GET failed (offline, a 5xx). Falling
      // through would take the `else` branch below and CREATE a second activity -- the user
      // asked to change one entry and would silently get two, with their edit on the copy.
      error = $t('activity.not-loaded');
      return;
    }
    const body = buildInput();
    const bad = validateActivity(body);
    if (bad) { error = fieldError(bad, $t); return; }
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
      // lost instead of navigating away as though it had been saved. `$t` on any other
      // (plain-English, server-supplied) message just returns it unchanged.
      error = $t((err as Error).message);
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
        try { await cancelQueuedActivity(saved.id); } catch { /* leaving it is better than blocking the exit */ }
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
          error = $t((e as Error).message);
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
      error = $t((e as Error).message);
      return;
    }
    go(`/objects/${oid}`, true);
  }
</script>

<main>
  <TopBar title={editing ? $t('activity.edit') : $t('activity.new')} backTo={`/objects/${oid}`} />
  <form onsubmit={submit}>
    <div class="row">
      <div class="field"><label for="d">{$t('activity.date')}</label><DateInput id="d" bind:value={input.date} required /></div>
      <div class="field">
        <label for="c">{$t('activity.category')}</label>
        <select id="c" bind:value={input.category} onchange={() => (categoryTouched = true)}>
          {#each offered as c}<option value={c}>{$t(`cat.${c}`)}</option>{/each}
        </select>
      </div>
    </div>
    {#if photoDate && photoDate !== input.date}
      <button type="button" class="ghost hintbtn" onclick={() => (input.date = photoDate)}>{$t('activity.use-exif-date', { date: fmtDate(photoDate, $dateFormat) })}</button>
    {/if}
    {#if !editing && suggestions.length > 0}
      <div class="chips">
        {#each suggestions.slice(0, 3) as s (s.title + s.category)}
          <button type="button" class="chip" onclick={() => repeat(s)}>{$t('activity.repeat')}: {s.title}</button>
        {/each}
      </div>
    {/if}
    <div class="field">
      <label for="ti">{$t('activity.title')}</label>
      <!-- Optional only for a trip or a charge (spec: "defaults to $t('cat.trip') when empty",
           and a charge to "Charged"/"Geladen" or the petrol wording) -- shown as the placeholder
           rather than pre-filled, so it stays plainly a hint and not text the user has to notice
           and delete. `activityTitle` (also what Timeline.svelte falls back to for an
           untitled row) is reused here so the two can never disagree about the fallback word. -->
      <input id="ti" list="titles" bind:value={input.title} required={input.category !== 'trip' && input.category !== 'fuel'}
             placeholder={activityTitle('', input.category, $t, object?.fuel_unit ?? undefined) || undefined} />
      <datalist id="titles">
        {#each suggestions as s (s.title + s.category)}<option value={s.title}></option>{/each}
      </datalist>
    </div>
    {#if input.category === 'fuel'}
      <label class="row toggle">
        <input type="checkbox" bind:checked={chargedFull} />
        {$t('activity.charged-full')}
      </label>
    {/if}
    {#if input.category === 'trip'}
      <!-- `object?.counter_unit` (not a plain `object.counter_unit`): an existing trip must
           still be editable offline even if the object itself failed to load (no cache either),
           the same reason `object?.counter_unit` gates the generic Counter field's `label` --
           these labels just show no unit in that rare case instead of throwing on `object.`. -->
      <div class="row">
        <div class="field">
          <label for="tst">{$t('trip.start')} ({object?.counter_unit ?? ''})</label>
          <input id="tst" type="number" inputmode="numeric" min="0" bind:value={input.start_counter} oninput={onTripStartChange} />
        </div>
        <div class="field">
          <label for="ten">{$t('trip.end')} ({object?.counter_unit ?? ''})</label>
          <input id="ten" type="number" inputmode="numeric" min="0" bind:value={input.counter_value} oninput={onTripEndChange} />
        </div>
      </div>
      <div class="field">
        <label for="tds">{$t('trip.distance')} ({object?.counter_unit ?? ''})</label>
        <!-- No `min="0"` (unlike Start/End): distance is `end - start`, so an end typed below
             start makes it negative -- exactly the mistake `trip.error-end` exists to explain.
             A native `min` would instead block the browser's own submit outright before that
             message ever runs, leaving Save looking like it silently does nothing.
             No `step` either, on purpose: the default (whole numbers only) matches Start/End,
             which are themselves whole counter units, so a decimal distance could never actually
             be reached by any real start/end pair -- typing one is simply refused by the field,
             the same way Start/End already refuse one. -->
        <input id="tds" type="number" inputmode="numeric" bind:value={distance} oninput={onTripDistanceChange} />
      </div>
      <div class="row">
        <div class="field">
          <label for="tfr">{$t('trip.from')}</label>
          <input id="tfr" list="trip-from" maxlength="80" bind:value={fromText} />
          <datalist id="trip-from">{#each tripPlaces.from as p (p)}<option value={p}></option>{/each}</datalist>
        </div>
        <div class="field">
          <label for="tto">{$t('trip.to')}</label>
          <input id="tto" list="trip-to" maxlength="80" bind:value={toText} />
          <datalist id="trip-to">{#each tripPlaces.to as p (p)}<option value={p}></option>{/each}</datalist>
        </div>
      </div>
      <div class="row">
        <div class="field">
          <label for="tdu">{$t('trip.duration')}</label>
          <input id="tdu" type="text" inputmode="numeric" placeholder="h:mm" bind:value={durationText} />
        </div>
        <div class="field">
          <label for="tba">{$t('trip.battery')} (%)</label>
          <!-- No `min`/`max`: same reason the Distance field below has no `min="0"` -- a native
               bound would block the browser's own submit before `validateActivity`'s own
               `trip.error-battery` message ever ran, so typing 101 would look like Save
               silently did nothing instead of showing that message. -->
          <input id="tba" type="number" inputmode="numeric" bind:value={input.battery_used_pct} />
        </div>
      </div>
    {/if}
    <div class="row">
      {#if object?.counter_unit && input.category !== 'trip'}
        <div class="field">
          <label for="cv">{$t('activity.counter')} ({object.counter_unit})</label>
          <input id="cv" type="number" inputmode="numeric" min="0" bind:value={counterText} />
          {#if counterWarn}<span class="warn">{$t('activity.counter-warn', { last: fmtCounter(lastCounter, object.counter_unit, $locale) })}</span>{/if}
        </div>
      {/if}
      <div class="field"><label for="co">{$t('activity.cost')}</label><input id="co" type="text" inputmode="decimal" bind:value={costText} /></div>
    </div>
    {#if input.category === 'fuel' && object?.counter_unit}
      <div class="field">
        <label for="qt">{$t('activity.quantity')} ({object.fuel_unit ? fuelUnitLabel(object.fuel_unit) : (object.counter_unit === 'mi' ? 'gal' : 'l')})</label>
        <input id="qt" type="text" inputmode="decimal" bind:value={quantityText} />
      </div>
    {/if}
    <div class="field"><label for="no">{$t('activity.notes')}</label><textarea id="no" bind:value={input.notes}></textarea></div>
    <TagInput bind:tags={() => input.tags ?? [], (v) => (input.tags = v)} suggestions={tagCounts} label={$t('tags.label')} id="tags" />

    <h2>{$t('activity.photos')}</h2>
    {#if attachments.length > 0}
      <div class="thumb-strip">
        {#each attachments as a (a.id)}
          <div class="strip-item" class:pending={a.pending}>
            {#if a.kind === 'photo'}
              <img src={a.pending ? a.previewUrl : fileUrl(a.file_id, true)} alt="" />
            {:else}
              <span class="doc-chip"><Icon name="document" size={28} /></span>
            {/if}
            {#if a.pending}<span class="chip pending-chip">{$t('timeline.pending')}</span>{/if}
          </div>
        {/each}
      </div>
    {/if}
    {#if saved}
      <FilePicker objectId={oid} activityId={saved.id} onuploaded={(a) => (attachments = [...attachments, a])} />
    {:else if ready}
      <button type="button" class="ghost pickerlike" onclick={async () => { try { await ensureSaved(); error = ''; } catch (e) { error = (e as Error).message; } }}>
        + {$t('activity.add-files')}
      </button>
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="row actions">
      <button type="button" class="ghost" onclick={cancel}>{$t('nav.cancel')}</button>
      <button class="primary" disabled={busy}>{$t('nav.save')}</button>
    </div>
  </form>
  {#if editing}
    <button class="danger" onclick={remove}>{$t('nav.delete')}</button>
  {/if}
</main>

<style>
  .hintbtn { font-size: var(--text-sm); color: var(--accent); padding: var(--space-1) 0; text-align: left; }
  .pickerlike { border: 1px dashed var(--border); width: 100%; }
  .doc-chip { display: grid; place-items: center; width: 64px; height: 64px; background: var(--surface-2); border-radius: var(--radius-sm); }
  .actions { margin-top: var(--space-2); }
  /* A positioning context for the pending badge, not a thumbnail. It was called `thumb`, which
     collided with the global grid-image rule in app.css and inflated it to a full-width square
     around a 64px image. */
  .strip-item { position: relative; flex: none; }
  .strip-item.pending { opacity: .55; }
  .strip-item .pending-chip { position: absolute; left: 2px; right: 2px; bottom: 2px; text-align: center; font-size: var(--text-xs); padding: 1px 2px; line-height: 1.2; }
</style>
