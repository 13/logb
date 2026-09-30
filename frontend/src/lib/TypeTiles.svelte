<script lang="ts">
  import Icon from './Icon.svelte';
  import { OBJECT_TYPES, type ObjectType } from './types';
  import { customTypes, typeIcon, typesLoaded } from './type-registry';
  import { errorClass, labelClass } from '$lib/components/ui/field/classes.js';
  import { t } from '../i18n';

  /**
   * The object's type as a grid of icon tiles, first on the form: the type decides the counter,
   * the templates and what the object can log. Native radios, invisible, each filling its tile.
   * Nothing is checked until `picked`: a new object has no type until the user chooses one.
   */
  let { value, picked, error = '', onpick, onnewtype }: {
    value: ObjectType; picked: boolean; error?: string; onpick: (type: ObjectType) => void; onnewtype: () => void;
  } = $props();

  /** An own type deleted elsewhere (not synced here yet) still shows as chosen, as "Unknown type". */
  const missing = $derived(value.startsWith('custom:') && !$customTypes.some((c) => c.key === value));
  const tile = 'relative flex min-h-18 cursor-pointer flex-col items-center justify-center gap-1 rounded-lg border border-input bg-card p-2 text-center text-xs font-medium text-foreground transition-colors hover:not-has-checked:bg-accent has-checked:border-brand-ink has-checked:bg-primary/10 has-checked:text-brand-ink has-focus-visible:outline-2 has-focus-visible:outline-solid has-focus-visible:outline-offset-2 has-focus-visible:outline-ring';
</script>

{#snippet option(key: string, label: string)}
  <label class={tile}>
    <input type="radio" data-slot="type-tile" name="object-type" value={key} checked={picked && value === key}
           onchange={() => onpick(key as ObjectType)}
           class="absolute inset-0 m-0 size-full cursor-pointer appearance-none rounded-lg opacity-0" />
    <Icon name={typeIcon(key, $customTypes)} size={22} />
    <span class="line-clamp-2 break-words">{label}</span>
  </label>
{/snippet}

<fieldset data-slot="type-tiles" class="m-0 min-w-0 border-0 p-0" aria-describedby={error ? 'object-type-error' : undefined}>
  <legend class={`${labelClass} mb-1.5 p-0`}>{$t('object.type')}</legend>
  <!-- tabindex -1: where a refused save moves the focus (`OBJECT_FIELD_IDS`). -->
  <div id="object-type" tabindex="-1" class="grid scroll-my-24 grid-cols-3 gap-2 outline-none desk:grid-cols-5">
    {#each OBJECT_TYPES as ty (ty)}{@render option(ty, $t(`type.${ty}`))}{/each}
    {#if missing}{@render option(value, $typesLoaded ? $t('types.unknown') : $t('types.loading'))}{/if}
  </div>
  {#if $customTypes.length > 0}
    <div role="group" aria-labelledby="object-type-yours" class="mt-3 flex flex-col gap-1.5">
      <span id="object-type-yours" class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">{$t('types.yours')}</span>
      <div class="grid grid-cols-3 gap-2 desk:grid-cols-5">
        {#each $customTypes as ct (ct.key)}{@render option(ct.key, ct.name)}{/each}
      </div>
    </div>
  {/if}
  <button type="button" data-slot="new-type" onclick={onnewtype}
          class="mt-1 min-h-11 cursor-pointer text-sm font-medium text-brand-ink underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring">{$t('types.new-from-form')}</button>
  {#if error}<p id="object-type-error" role="alert" class={errorClass}>{error}</p>{/if}
</fieldset>
