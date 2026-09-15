<script lang="ts">
  import { untrack } from 'svelte';
  import { t, locale } from '../i18n';
  import { dateFormat } from '../stores/date-format';
  import { datePlaceholder, fmtDate, parseDate } from './format';
  import Icon from './Icon.svelte';

  let { id, value = $bindable(''), min, max, required = false, label }:
    { id: string; value?: string; min?: string; max?: string; required?: boolean; label?: string } = $props();

  let text = $state(fmtDate(value, $dateFormat));
  let invalid = $state(false);
  let picker: HTMLInputElement;
  let field: HTMLInputElement;

  const errorMessage = $derived($t('date.invalid', { example: fmtDate('2026-09-15', $dateFormat) }));

  // The browser's own constraint validation refuses to fire the form's `submit` event while any
  // field carries a custom validity message -- which is what makes "saving keeps the user on
  // the form" true without the parent needing to know this field is invalid: nothing to wire up,
  // because the browser itself withholds the submit.
  function markInvalid(bad: boolean) {
    invalid = bad;
    field?.setCustomValidity(bad ? errorMessage : '');
  }

  // An outside change (a picked photo date, a reset form) must show in the field. `text` is read
  // through `untrack` so this reacts only to `value`/`$dateFormat` changing from outside -- not
  // to the writes this effect's own dependents (typing, `commit`) make to `text` itself, which
  // would otherwise re-run this same effect on every keystroke and silently revert whatever was
  // just typed before it could ever be blurred.
  $effect(() => {
    const shown = untrack(() => parseDate(text, $dateFormat));
    if ((value || null) !== shown) { text = fmtDate(value, $dateFormat); markInvalid(false); }
  });

  function commit() {
    if (text.trim() === '') { value = ''; markInvalid(false); return; }
    const parsed = parseDate(text, $dateFormat);
    const outOfRange = parsed !== null && ((min && parsed < min) || (max && parsed > max));
    markInvalid(parsed === null || !!outOfRange);
    if (!invalid && parsed) { value = parsed; text = fmtDate(parsed, $dateFormat); }
  }

  function openPicker() {
    picker.value = value;
    try { picker.showPicker(); } catch { picker.focus(); }
  }
</script>

<span class="date-input">
  <input {id} bind:this={field} type="text" inputmode="numeric" autocomplete="off" {required} aria-label={label}
         placeholder={datePlaceholder($dateFormat, $locale)} bind:value={text}
         aria-invalid={invalid} aria-describedby={invalid ? `${id}-error` : undefined}
         onblur={commit} onchange={commit} />
  <button type="button" class="ghost" aria-label={$t('date.pick')} onclick={openPicker}><Icon name="calendar" size={18} /></button>
  <input bind:this={picker} class="picker" type="date" tabindex="-1" aria-hidden="true" {min} {max}
         onchange={() => { value = picker.value; text = fmtDate(picker.value, $dateFormat); markInvalid(false); }} />
</span>
{#if invalid}
  <span id={`${id}-error`} class="error" aria-live="polite">{errorMessage}</span>
{/if}

<style>
  .date-input { display: flex; gap: var(--space-1); align-items: center; position: relative; }
  .date-input input[type='text'] { flex: 1; min-width: 0; }
  /* Kept in the layout (not display:none) so showPicker() has an anchor to open from. */
  .picker { position: absolute; right: 0; bottom: 0; width: 1px; height: 1px; opacity: 0; pointer-events: none; }
</style>
