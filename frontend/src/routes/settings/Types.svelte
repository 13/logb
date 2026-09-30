<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import Icon, { type IconName } from '../../lib/Icon.svelte';
  import { api, ApiError } from '../../lib/api';
  import { errorMessage } from '../../lib/api-error';
  import { newOpId } from '../../lib/outbox';
  import { t } from '../../i18n';
  import { go } from '../../lib/router';
  import { safeReturnPath } from '../../lib/object-draft';
  import { Button } from '$lib/components/ui/button/index.js';
  import { CheckField, Field } from '$lib/components/ui/field/index.js';
  import { Input } from '$lib/components/ui/input/index.js';
  import { NativeSelect } from '$lib/components/ui/native-select/index.js';
  import { destructiveGhostClass, errorClass, labelClass, sectionHeadingClass } from '$lib/components/ui/field/classes.js';
  import { CUSTOM_TYPE_ICONS, customTypes, loadCustomTypes, typeIcon } from '../../lib/type-registry';
  import { CATEGORIES, OBJECT_TYPES, type Category, type CounterUnit, type CustomType } from '../../lib/types';

  /** `null`: no form open. `'new'`: the add form. A number: that type's id, edited in place. */
  let editing = $state<'new' | number | null>(null);
  let name = $state('');
  let icon = $state<IconName>('object');
  let categories = $state<Category[]>([]);
  let unit = $state<CounterUnit>(null);
  let busy = $state(false);
  let formError = $state('');
  /** A refused delete is about one row, so its message sits under that row. */
  let deleteError = $state<{ id: number; message: string } | null>(null);
  /** Set from `?return=` on mount: the object form the "+ New type…" shortcut came from. A
   *  successful create sends the new type's key back there; Cancel on the add form it opened
   *  goes back without one. Absent (`null`) leaves both unchanged from Types opened normally. */
  let returnPath = $state<string | null>(null);
  /** The one-time token that came with `return`, carried on unchanged back to the object form
   *  (`draft=`) on both Save and Cancel -- see `object-draft.ts`. Not validated here: this page
   *  only ferries it, the object form is what checks it matches before trusting anything back. */
  let draftToken = $state<string | null>(null);

  onMount(() => {
    void loadCustomTypes();
    const params = new URLSearchParams(location.search);
    returnPath = safeReturnPath(params.get('return'));
    draftToken = params.get('draft');
    if (params.get('new') === '1') open('new');
  });

  /** `returnPath` with `extra` query params plus the carried `draft` token, if any -- the one
   *  place both Save and Cancel build the address they send the shortcut back to. */
  function returnUrl(extra: Record<string, string> = {}): string {
    const qs = new URLSearchParams(extra);
    if (draftToken !== null) qs.set('draft', draftToken);
    const s = qs.toString();
    return s ? `${returnPath}?${s}` : returnPath!;
  }

  /** Built-in labels already exist for most icons; the rest have their own word. */
  const ICON_LABEL: Partial<Record<IconName, string>> = {
    car: 'type.car', 'e-bike': 'type.e_bike', bike: 'type.bike', motorcycle: 'type.motorcycle', home: 'type.home',
    appliance: 'type.appliance', tool: 'type.tool', body: 'type.body',
  };
  const iconLabel = (i: IconName) => $t(ICON_LABEL[i] ?? `icon.${i}`);

  function open(target: 'new' | CustomType) {
    formError = ''; deleteError = null;
    if (target === 'new') {
      editing = 'new'; name = ''; icon = 'object'; unit = null;
      categories = ['maintenance', 'repair', 'inspection', 'purchase', 'other'];
    } else {
      editing = target.id; name = target.name; icon = target.icon; unit = target.counter_unit;
      categories = [...target.categories];
    }
  }

  function toggle(c: Category, on: boolean) {
    categories = on ? [...categories, c] : categories.filter((x) => x !== c);
  }

  async function save() {
    busy = true; formError = '';
    // In `CATEGORIES` order, whatever order they were ticked in; `other` always, since it is the
    // category that fits any entry and the server adds it anyway.
    const body = { name: name.trim(), icon, categories: CATEGORIES.filter((c) => c === 'other' || categories.includes(c)), counter_unit: unit };
    try {
      if (editing === 'new') {
        const created = await api<CustomType>('POST', '/types', { ...body, client_uuid: newOpId() });
        await loadCustomTypes();
        editing = null;
        // Send the "+ New type…" shortcut back to its form with the type it just made --
        // `created.key` (the registry's own `custom:<uuid>`), not anything derived here.
        // Replace, not push: Back from the saved object must not land on this add form again.
        if (returnPath) { go(returnUrl({ type: created.key }), true); return; }
      } else {
        await api('PATCH', `/types/${editing}`, body);
        await loadCustomTypes();
        editing = null;
      }
    } catch (e) {
      formError = errorText(e);
    } finally { busy = false; }
  }

  /** Cancel on the add form the shortcut opened goes back to where it came from, type
   *  unchanged; any other cancel (editing a type, or Types opened without `return`) just closes
   *  the form in place, as before. */
  function cancelForm() {
    if (editing === 'new' && returnPath) { go(returnUrl(), true); return; }
    editing = null;
  }

  /** The type being deleted: its row is busy and no second delete starts meanwhile. */
  let removing = $state<number | null>(null);

  async function remove(ty: CustomType) {
    if (removing !== null) return;
    if (!confirm($t('nav.confirm-delete'))) return;
    deleteError = null;
    removing = ty.id;
    try {
      await api('DELETE', `/types/${ty.id}`);
      if (editing === ty.id) editing = null;
      await loadCustomTypes();
    } catch (e) {
      const count = e instanceof ApiError && e.code === 'in_use' ? Number(e.body?.count) : NaN;
      const message = Number.isFinite(count)
        ? (count === 1 ? $t('types.in-use-one') : $t('types.in-use', { n: count }))
        : errorText(e);
      deleteError = { id: ty.id, message };
    } finally { removing = null; }
  }

  /** The server's stable codes for a refused type (`types.error.*`), said in the reader's
   *  language -- `errorMessage` knows them along with every other stable code. */
  const errorText = (e: unknown): string => errorMessage(e, $t);

  const summary = (ty: CustomType) =>
    [ty.counter_unit, ty.categories.map((c) => $t(`cat.${c}`)).join(', ')].filter(Boolean).join(' · ');

  // Icon tiles as TypeTiles draws the object form's: the invisible radio fills its label, which
  // carries the look (brand-ink on primary/10 over card when checked, tested).
  const iconTile = 'relative flex min-h-12 cursor-pointer items-center justify-center rounded-lg border border-input bg-card text-muted-foreground transition-colors hover:not-has-checked:bg-accent has-checked:border-brand-ink has-checked:bg-primary/10 has-checked:text-brand-ink has-focus-visible:outline-2 has-focus-visible:outline-solid has-focus-visible:outline-offset-2 has-focus-visible:outline-ring';
</script>

{#snippet form()}
  <form data-testid="type-form" aria-busy={busy} class="m-0 flex flex-col gap-4 rounded-lg border border-border bg-card p-4 shadow-xs"
        onsubmit={(e) => { e.preventDefault(); void save(); }}>
    <Field id="tn" label={$t('types.name')}>
      <Input bind:value={name} maxlength={40} autocomplete="off" required />
    </Field>
    <fieldset class="m-0 flex min-w-0 flex-col border-0 p-0">
      <legend class={`${labelClass} mb-1.5 p-0`}>{$t('types.icon')}</legend>
      <div class="grid grid-cols-[repeat(auto-fill,minmax(3rem,1fr))] gap-2">
        {#each CUSTOM_TYPE_ICONS as i (i)}
          <label data-testid="icon-choice" class={iconTile} title={iconLabel(i)}>
            <input type="radio" data-slot="icon-radio" name="type-icon" value={i} bind:group={icon}
                   class="absolute inset-0 m-0 size-full cursor-pointer appearance-none rounded-lg opacity-0" />
            <Icon name={i} size={24} />
            <span class="sr-only">{iconLabel(i)}</span>
          </label>
        {/each}
      </div>
    </fieldset>
    <fieldset class="m-0 flex min-w-0 flex-col border-0 p-0">
      <legend class={`${labelClass} mb-1.5 p-0`}>{$t('types.categories')}</legend>
      <div class="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-3">
        {#each CATEGORIES as c (c)}
          <!-- "Other" fits any entry, so it is always on and cannot be taken off. -->
          <CheckField id={`type-cat-${c}`} label={$t(`cat.${c}`)} disabled={c === 'other'}
                      bind:checked={() => c === 'other' || categories.includes(c), (on) => toggle(c, on)} />
        {/each}
      </div>
    </fieldset>
    <Field id="tu" label={$t('types.unit')}>
      <NativeSelect bind:value={unit}>
        <option value={null}>{$t('types.unit-none')}</option>
        <option value="km">{$t('object.counter-km')}</option>
        <option value="mi">{$t('object.counter-mi')}</option>
        <option value="h">{$t('object.counter-h')}</option>
      </NativeSelect>
    </Field>
    {#if formError}<p role="alert" class={errorClass}>{formError}</p>{/if}
    <div class="flex gap-2">
      <!-- While the form is open, "Add type" is hidden: this is the page's one primary. -->
      <Button type="submit" class="h-12 flex-1 sm:min-w-28 sm:flex-none" disabled={busy || name.trim() === ''}>{$t('types.save')}</Button>
      <Button variant="outline" class="h-12 flex-1 sm:min-w-28 sm:flex-none" onclick={cancelForm}>{$t('nav.cancel')}</Button>
    </div>
  </form>
{/snippet}

<main>
  <TopBar title={$t('settings.types')} backTo="/settings" />
  <div class="mx-auto flex w-full max-w-2xl flex-col gap-6">
    <section class="flex flex-col gap-2">
      <h2 id="types-yours" class={sectionHeadingClass}>{$t('types.yours')}</h2>
      <!-- No list at all while the first type is being added: an empty labelled list would be
           announced as "Your types, list, 0 items" above the form. -->
      {#if $customTypes.length > 0 || editing !== 'new'}
      <ul role="list" aria-labelledby="types-yours" class="m-0 flex list-none flex-col gap-2 p-0">
        {#each $customTypes as ty (ty.id)}
          <li>
            {#if editing === ty.id}
              {@render form()}
            {:else}
              <div data-testid="type-row" aria-busy={removing === ty.id} class="grid grid-cols-[auto_minmax(0,1fr)_auto_auto] items-center gap-2 rounded-lg border border-border bg-card p-3 shadow-xs">
                <span class="grid size-10 place-items-center rounded-md bg-primary/10 text-brand-ink" aria-hidden="true"><Icon name={ty.icon} /></span>
                <span class="flex min-w-0 flex-col">
                  <b class="font-semibold text-foreground [overflow-wrap:anywhere]">{ty.name}</b>
                  <span class="truncate text-sm text-muted-foreground">{summary(ty)}</span>
                </span>
                <Button variant="ghost" class="min-h-11" aria-label={$t('types.edit-named', { name: ty.name })} onclick={() => open(ty)}>{$t('nav.edit')}</Button>
                <Button variant="ghost" class={`min-h-11 ${destructiveGhostClass}`} aria-label={$t('types.delete-named', { name: ty.name })} disabled={removing !== null} onclick={() => remove(ty)}>{$t('types.delete')}</Button>
                {#if deleteError?.id === ty.id}<p role="alert" class={`${errorClass} col-span-full`}>{deleteError.message}</p>{/if}
              </div>
            {/if}
          </li>
        {:else}
          <li class="px-1 py-4 text-sm text-muted-foreground">{$t('types.empty')}</li>
        {/each}
      </ul>
      {/if}
      {#if editing === 'new'}
        {@render form()}
      {:else}
        <!-- The page's one primary action. -->
        <Button class="h-12 self-start" onclick={() => open('new')}>{$t('types.add')}</Button>
      {/if}
    </section>

    <section class="flex flex-col gap-2">
      <h2 id="types-built-in" class={sectionHeadingClass}>{$t('types.built-in')}</h2>
      <ul role="list" aria-labelledby="types-built-in" class="m-0 grid list-none grid-cols-1 gap-x-4 rounded-lg border border-border bg-card p-3 shadow-xs sm:grid-cols-2">
        {#each OBJECT_TYPES as ty (ty)}
          <li class="flex min-h-11 items-center gap-3 text-foreground"><span class="text-muted-foreground" aria-hidden="true"><Icon name={typeIcon(ty, [])} /></span>{$t(`type.${ty}`)}</li>
        {/each}
      </ul>
    </section>
  </div>
</main>
