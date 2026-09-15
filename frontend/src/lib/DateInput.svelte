<script lang="ts">
  import { untrack } from 'svelte';
  import { t, locale } from '../i18n';
  import { dateFormat } from '../stores/date-format';
  import { datePlaceholder, fmtDate, parseDate, type DateFormat } from './format';
  import Icon from './Icon.svelte';

  let { id, value = $bindable(''), min, max, required = false, label }:
    { id: string; value?: string; min?: string; max?: string; required?: boolean; label?: string } = $props();

  let text = $state(fmtDate(value, $dateFormat));
  /** The showing error message, or `null` when the field is valid. A string (not a boolean),
   *  because there are two different messages to show: an unparsable date, or a parsed-but-out-
   *  of-range one. */
  let error = $state<string | null>(null);
  let focused = $state(false);
  let picker: HTMLInputElement;
  let field: HTMLInputElement;

  function invalidMessage(fmt: DateFormat): string {
    return $t('date.invalid', { example: fmtDate('2026-09-15', fmt) });
  }

  /** `iso` (`YYYY-MM-DD`) string comparison sorts the same as calendar order, so `<`/`>` on the
   *  raw strings is enough -- no need to parse either bound. */
  function rangeMessage(iso: string, fmt: DateFormat): string | null {
    const belowMin = !!min && iso < min;
    const aboveMax = !!max && iso > max;
    if (!belowMin && !aboveMax) return null;
    if (min && max) return $t('date.out-of-range', { min: fmtDate(min, fmt), max: fmtDate(max, fmt) });
    if (max) return $t('date.before', { max: fmtDate(max, fmt) });
    return $t('date.after', { min: fmtDate(min, fmt) });
  }

  /** The message committing `raw` right now, under `fmt`, would produce -- shared by `commit()`,
   *  the hidden picker's `onchange`, and the effect that refreshes an already-showing message
   *  when the date-format setting changes underneath it. */
  function messageFor(raw: string, fmt: DateFormat): string | null {
    if (raw.trim() === '') return null;
    const parsed = parseDate(raw, fmt);
    return parsed === null ? invalidMessage(fmt) : rangeMessage(parsed, fmt);
  }

  // The browser's own constraint validation refuses to fire the form's `submit` event while any
  // field carries a custom validity message -- which is what makes "saving keeps the user on
  // the form" true without the parent needing to know this field is invalid: nothing to wire up,
  // because the browser itself withholds the submit.
  function markError(msg: string | null) {
    error = msg;
    field?.setCustomValidity(msg ?? '');
  }

  // Keeps the field showing the canonical rendering of `value` whenever nothing more urgent is
  // going on: `value`/`$dateFormat` are read unconditionally (so an outside change -- a picked
  // photo date, a different activity loading, the format setting itself changing -- is never
  // missed), but the write only happens while the field is not focused (never fights typing) and
  // while there is no error showing (a blurred, still-wrong date stays on screen with its error
  // until the user fixes it, rather than silently reverting to the last valid value).
  $effect(() => {
    const canonical = fmtDate(value, $dateFormat);
    if (focused) return;
    if (untrack(() => error) !== null) return;
    if (text !== canonical) text = canonical;
  });

  // A showing message embeds a formatted date (the invalid-input example, or the min/max in an
  // out-of-range one) -- refresh it if the date-format setting changes while it is up, even
  // mid-edit. `text`/`error` are read through `untrack` so typing alone never re-runs this.
  $effect(() => {
    const fmt = $dateFormat;
    if (untrack(() => error) === null) return;
    markError(untrack(() => messageFor(text, fmt)));
  });

  function commit() {
    if (text.trim() === '') { value = ''; markError(null); return; }
    const parsed = parseDate(text, $dateFormat);
    if (parsed === null) { markError(invalidMessage($dateFormat)); return; }
    const range = rangeMessage(parsed, $dateFormat);
    if (range) { markError(range); return; }
    markError(null);
    value = parsed; text = fmtDate(parsed, $dateFormat);
  }

  function openPicker() {
    picker.value = value;
    // `field`, not `picker`: `picker` is `aria-hidden`, so focusing it as a fallback would move
    // focus somewhere a screen reader is told does not exist.
    try { picker.showPicker(); } catch { field.focus(); }
  }

  function onPickerChange() {
    const picked = picker.value;
    // Reset immediately, win or lose: a native date input enforces its OWN `min`/`max` (were any
    // bound here) against whatever value it last holds, even while hidden -- so leaving a stale
    // one on it is a way for this control to silently fail its own constraint validation and
    // block the form's submit with nothing on screen explaining why. Emptied, and with no
    // `min`/`max` bound below, it can never do that.
    picker.value = '';
    if (!picked) return; // the picker's own "Clear" control
    const range = rangeMessage(picked, $dateFormat);
    markError(range);
    if (!range) { value = picked; text = fmtDate(picked, $dateFormat); }
  }
</script>

<span class="date-input">
  <input {id} bind:this={field} type="text" inputmode="numeric" autocomplete="off" {required} aria-label={label}
         placeholder={datePlaceholder($dateFormat, $locale)} bind:value={text}
         aria-invalid={!!error} aria-describedby={error ? `${id}-error` : undefined}
         onfocus={() => (focused = true)} onblur={() => { focused = false; commit(); }} onchange={commit} />
  <button type="button" class="ghost" aria-label={$t('date.pick')} onclick={openPicker}><Icon name="calendar" size={18} /></button>
  <!-- No `min`/`max` here (see `onPickerChange`'s comment): range is enforced by this component's
       own `rangeMessage`, identically for a typed date and a picked one. -->
  <input bind:this={picker} class="picker" type="date" tabindex="-1" aria-hidden="true" onchange={onPickerChange} />
</span>
{#if error}
  <span id={`${id}-error`} class="error" aria-live="polite">{error}</span>
{/if}

<style>
  .date-input { display: flex; gap: var(--space-1); align-items: center; position: relative; }
  .date-input input[type='text'] { flex: 1; min-width: 0; }
  /* Kept in the layout (not display:none) so showPicker() has an anchor to open from. */
  .picker { position: absolute; right: 0; bottom: 0; width: 1px; height: 1px; opacity: 0; pointer-events: none; }
</style>
