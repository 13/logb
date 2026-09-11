<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import TopBar from '../lib/TopBar.svelte';
  import FilePicker from '../lib/FilePicker.svelte';
  import Icon from '../lib/Icon.svelte';
  import { api, cancelQueuedActivity, createQueued, fileUrl, isRejection, onOutboxFlushed, updateQueuedActivity } from '../lib/api';
  import { newOpId, serialize } from '../lib/outbox';
  import { getCachedObject, setCachedObject } from '../lib/object-cache';
  import { go, back } from '../lib/router';
  import { centsToInput, counter as fmtCounter, fmtDate, parseMoney, parseQuantity } from '../lib/format';
  import { emptyActivity, exifDate, suggestionsFor, toActivityInput, validateActivity } from '../lib/activity-form';
  import { locale, t } from '../i18n';
  import { CATEGORIES, type Activity, type Attachment, type MemObject, type ActivityInput, type TitleSuggestion } from '../lib/types';

  let { id, aid }: { id: string; aid?: string } = $props();
  const oid = $derived(Number(id));
  const editing = $derived(aid !== undefined);

  let object = $state<MemObject | null>(null);
  let input = $state<ActivityInput>(emptyActivity());
  let costText = $state('');
  let counterText = $state('');
  let quantityText = $state('');
  let attachments = $state<Attachment[]>([]);
  let saved = $state<Activity | null>(null);
  let error = $state('');
  let busy = $state(false);
  let allSuggestions = $state<TitleSuggestion[]>([]);
  const suggestions = $derived(suggestionsFor(allSuggestions, input.category));
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

  onMount(async () => {
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
    const body = buildInput();
    const bad = validateActivity(body);
    if (bad) throw new Error($t(bad));
    const tempId = mintTempId();
    // `createQueued` returns null when the write only reached the outbox (offline). Passing
    // the SAME tempId as its `tempId` argument means the outbox stores it on the queued op, so
    // an upload queued below against this id gets rewritten to the real one once the create
    // lands (see `persistResolvedId` in ../lib/outbox.ts) -- not a second, unrelated numbering
    // scheme invented here.
    const result = await createQueued<Activity>(`/objects/${oid}/activities`, body as unknown as Record<string, unknown>, tempId);
    saved = result ?? {
      id: tempId, object_id: oid, ...body,
      created_at: new Date().toISOString(), updated_at: new Date().toISOString(),
      attachments: [], pending: true,
    };
    autoDraft = true;
    return saved;
  });

  function buildInput(): ActivityInput {
    return {
      ...input,
      cost_cents: parseMoney(costText),
      // `counterText` is bound to a number input, so Svelte hands back a number, not a string.
      counter_value: String(counterText).trim() === '' ? null : Number(counterText),
      // The quantity field only exists in the form for the fuel category (see the template
      // below) -- send it only then, so switching category away from fuel after typing an
      // amount can't leave a fuel quantity stuck on a repair/maintenance/... row.
      // Same comma/dot handling as parseMoney, so this field and cost agree on what's valid input.
      quantity_milli: input.category === 'fuel' ? parseQuantity(quantityText) : null,
    };
  }

  /** Prefill from a past entry. The user still reviews and saves; nothing is written here. */
  function repeat(s: TitleSuggestion) {
    input.title = s.title;
    input.category = s.category;
    if (s.last_cost_cents !== null) costText = centsToInput(s.last_cost_cents);
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
    if (bad) { error = $t(bad); return; }
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
        await api('PATCH', `/activities/${saved.id}`, body);
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
      <div class="field"><label for="d">{$t('activity.date')}</label><input id="d" type="date" bind:value={input.date} required /></div>
      <div class="field">
        <label for="c">{$t('activity.category')}</label>
        <select id="c" bind:value={input.category}>
          {#each CATEGORIES as c}<option value={c}>{$t(`cat.${c}`)}</option>{/each}
        </select>
      </div>
    </div>
    {#if photoDate && photoDate !== input.date}
      <button type="button" class="ghost hintbtn" onclick={() => (input.date = photoDate)}>{$t('activity.use-exif-date', { date: fmtDate(photoDate, $locale) })}</button>
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
      <input id="ti" list="titles" bind:value={input.title} required />
      <datalist id="titles">
        {#each suggestions as s (s.title + s.category)}<option value={s.title}></option>{/each}
      </datalist>
    </div>
    <div class="row">
      {#if object?.counter_unit}
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
        <label for="qt">{$t('activity.quantity')} ({object.fuel_unit ?? (object.counter_unit === 'mi' ? 'gal' : 'l')})</label>
        <input id="qt" type="text" inputmode="decimal" bind:value={quantityText} />
      </div>
    {/if}
    <div class="field"><label for="no">{$t('activity.notes')}</label><textarea id="no" bind:value={input.notes}></textarea></div>

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
      <button type="button" class="ghost pickerlike" onclick={async () => { try { await ensureSaved(); } catch (e) { error = (e as Error).message; } }}>
        + {$t('activity.add-files')}
      </button>
    {/if}

    {#if error}<p class="error">{error}</p>{/if}
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
  .hintbtn { font-size: .85rem; color: var(--accent); padding: 4px 0; text-align: left; }
  .pickerlike { border: 1px dashed var(--border); width: 100%; }
  .doc-chip { display: grid; place-items: center; width: 64px; height: 64px; background: var(--surface-2); border-radius: 6px; }
  .actions { margin-top: 8px; }
  /* A positioning context for the pending badge, not a thumbnail. It was called `thumb`, which
     collided with the global grid-image rule in app.css and inflated it to a full-width square
     around a 64px image. */
  .strip-item { position: relative; flex: none; }
  .strip-item.pending { opacity: .55; }
  .strip-item .pending-chip { position: absolute; left: 2px; right: 2px; bottom: 2px; text-align: center; font-size: .6rem; padding: 1px 2px; line-height: 1.2; }
</style>
