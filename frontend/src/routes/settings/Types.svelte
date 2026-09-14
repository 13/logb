<script lang="ts">
  import { onMount } from 'svelte';
  import TopBar from '../../lib/TopBar.svelte';
  import Icon, { type IconName } from '../../lib/Icon.svelte';
  import { api, ApiError } from '../../lib/api';
  import { newOpId } from '../../lib/outbox';
  import { t } from '../../i18n';
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

  onMount(() => { void loadCustomTypes(); });

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
      if (editing === 'new') await api('POST', '/types', { ...body, client_uuid: newOpId() });
      else await api('PATCH', `/types/${editing}`, body);
      await loadCustomTypes();
      editing = null;
    } catch (e) {
      formError = errorText(e);
    } finally { busy = false; }
  }

  async function remove(ty: CustomType) {
    if (!confirm($t('nav.confirm-delete'))) return;
    deleteError = null;
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
    }
  }

  /** The server's stable codes for a refused type, said in the reader's language. Anything else
   *  (a code this page does not know, a dropped connection) keeps the server's own sentence. */
  const TYPE_ERRORS = ['name_taken', 'name_invalid', 'icon_invalid', 'categories_invalid', 'unit_invalid'];
  function errorText(e: unknown): string {
    if (e instanceof ApiError && TYPE_ERRORS.includes(e.code)) return $t(`types.error.${e.code}`);
    return (e as Error).message;
  }

  const summary = (ty: CustomType) =>
    [ty.counter_unit, ty.categories.map((c) => $t(`cat.${c}`)).join(', ')].filter(Boolean).join(' · ');
</script>

{#snippet form()}
  <form class="card type-form" onsubmit={(e) => { e.preventDefault(); void save(); }}>
    <div class="field">
      <label for="tn">{$t('types.name')}</label>
      <input id="tn" bind:value={name} maxlength="40" autocomplete="off" required />
    </div>
    <fieldset class="field">
      <legend>{$t('types.icon')}</legend>
      <div class="icons">
        {#each CUSTOM_TYPE_ICONS as i (i)}
          <label class="icon-choice" title={iconLabel(i)}>
            <input type="radio" name="type-icon" value={i} bind:group={icon} class="visually-hidden" />
            <Icon name={i} size={24} />
            <span class="visually-hidden">{iconLabel(i)}</span>
          </label>
        {/each}
      </div>
    </fieldset>
    <fieldset class="field">
      <legend>{$t('types.categories')}</legend>
      <div class="cats">
        {#each CATEGORIES as c (c)}
          <label class="row toggle">
            <input type="checkbox" checked={c === 'other' || categories.includes(c)} disabled={c === 'other'}
              onchange={(e) => toggle(c, e.currentTarget.checked)} />
            {$t(`cat.${c}`)}
          </label>
        {/each}
      </div>
    </fieldset>
    <div class="field">
      <label for="tu">{$t('types.unit')}</label>
      <select id="tu" bind:value={unit}>
        <option value={null}>{$t('types.unit-none')}</option>
        <option value="km">{$t('object.counter-km')}</option>
        <option value="mi">{$t('object.counter-mi')}</option>
        <option value="h">{$t('object.counter-h')}</option>
      </select>
    </div>
    {#if formError}<p class="error" role="alert">{formError}</p>{/if}
    <div class="row actions">
      <button type="submit" class="primary" disabled={busy || name.trim() === ''}>{$t('types.save')}</button>
      <button type="button" class="ghost" onclick={() => (editing = null)}>{$t('nav.cancel')}</button>
    </div>
  </form>
{/snippet}

<main>
  <TopBar title={$t('settings.types')} backTo="/settings" />

  <h2>{$t('types.yours')}</h2>
  <div class="list">
    {#each $customTypes as ty (ty.id)}
      {#if editing === ty.id}
        {@render form()}
      {:else}
        <div class="card type-row">
          <span class="icon"><Icon name={ty.icon} /></span>
          <span class="text">
            <b>{ty.name}</b>
            <span class="muted summary">{summary(ty)}</span>
          </span>
          <button class="ghost" aria-label={$t('types.edit-named', { name: ty.name })} onclick={() => open(ty)}>{$t('nav.edit')}</button>
          <button class="ghost danger-text" aria-label={$t('types.delete-named', { name: ty.name })} onclick={() => remove(ty)}>{$t('types.delete')}</button>
          {#if deleteError?.id === ty.id}<p class="error full" role="alert">{deleteError.message}</p>{/if}
        </div>
      {/if}
    {:else}
      {#if editing !== 'new'}<div class="empty"><p>{$t('types.empty')}</p></div>{/if}
    {/each}
    {#if editing === 'new'}
      {@render form()}
    {:else}
      <button onclick={() => open('new')}>{$t('types.add')}</button>
    {/if}
  </div>

  <h2>{$t('types.built-in')}</h2>
  <ul class="card builtins">
    {#each OBJECT_TYPES as ty (ty)}
      <li><span class="icon"><Icon name={typeIcon(ty, [])} /></span>{$t(`type.${ty}`)}</li>
    {/each}
  </ul>
</main>

<style>
  .danger-text { color: var(--danger); }
  .type-row {
    display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto;
    align-items: center; gap: var(--space-2);
  }
  .type-row button { padding-inline: var(--space-2); }
  .icon { display: flex; color: var(--muted); }
  .text { display: flex; flex-direction: column; min-width: 0; }
  .text b { overflow-wrap: anywhere; }
  .summary { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .full { grid-column: 1 / -1; margin: 0; }
  fieldset { border: 0; padding: 0; margin-inline: 0; min-width: 0; }
  legend { font-size: var(--text-sm); color: var(--muted); padding: 0; margin-bottom: var(--space-1); }
  .icons { display: grid; grid-template-columns: repeat(auto-fill, minmax(48px, 1fr)); gap: var(--space-2); }
  .icon-choice {
    display: flex; align-items: center; justify-content: center; min-height: 48px;
    border: 1px solid var(--border); border-radius: var(--radius-sm); cursor: pointer; color: var(--muted);
  }
  .icon-choice:has(input:checked) { border-color: var(--accent); background: var(--accent); color: var(--accent-text); }
  .icon-choice:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  .cats { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 0 var(--space-3); }
  .cats label { min-height: 44px; }
  .actions { margin-top: var(--space-2); }
  .builtins { list-style: none; margin: 0; padding-block: var(--space-1); display: grid; }
  .builtins li { display: flex; align-items: center; gap: var(--space-3); min-height: 44px; }
  .visually-hidden {
    position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden;
    clip: rect(0 0 0 0); white-space: nowrap; border: 0;
  }
</style>
