<script lang="ts">
  import { untrack } from 'svelte';
  import { t, locale } from '../i18n';
  import { dateFormat } from '../stores/date-format';
  import { settings } from '../stores/settings';
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
  let field: HTMLInputElement;
  let pickerOpen = $state(false);
  let view = $state(new Date());

  const weekStartsMonday = $derived($settings.firstDayOfWeek === 'monday' || ($settings.firstDayOfWeek === 'locale' && $locale === 'de'));
  const weekdays = $derived(Array.from({ length: 7 }, (_, i) => {
    const offset = (i + (weekStartsMonday ? 1 : 0)) % 7;
    return new Intl.DateTimeFormat($locale, { weekday: 'short', timeZone: 'UTC' }).format(new Date(Date.UTC(2026, 7, 2 + offset)));
  }));
  const calendarDays = $derived.by(() => {
    const y = view.getUTCFullYear(), m = view.getUTCMonth();
    const first = new Date(Date.UTC(y, m, 1));
    const lead = (first.getUTCDay() - (weekStartsMonday ? 1 : 0) + 7) % 7;
    return Array.from({ length: 42 }, (_, i) => new Date(Date.UTC(y, m, i - lead + 1)));
  });
  const monthLabel = $derived(new Intl.DateTimeFormat($locale, { month: 'long', year: 'numeric', timeZone: 'UTC' }).format(view));

  /** `value` as this component last set it itself (in `commit()` or the picker's `onchange`).
   *  What tells the reformat effect below a change is genuinely from OUTSIDE -- a picked photo
   *  date, a different activity/template loading -- as opposed to the effect merely reacting to
   *  its own component's own write a moment before. */
  let lastSeenValue = value;

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

  /** The message committing `raw` right now, under `fmt`, would produce -- shared by `commit()`
   *  and the effect that refreshes an already-showing message when the date-format setting
   *  changes underneath it. */
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

  function commit() {
    if (text.trim() === '') { value = ''; lastSeenValue = ''; markError(null); return; }
    const parsed = parseDate(text, $dateFormat);
    if (parsed === null) { markError(invalidMessage($dateFormat)); return; }
    const range = rangeMessage(parsed, $dateFormat);
    if (range) { markError(range); return; }
    markError(null);
    value = parsed; lastSeenValue = parsed; text = fmtDate(parsed, $dateFormat);
  }

  // `value`/`$dateFormat` are read unconditionally, so neither an outside change nor a format
  // change is ever missed. Two different things can have happened, and they get different
  // treatment:
  //  - `value` itself moved since this component last set it (a picked photo date, a different
  //    activity or template loading): that is new ground truth, shown immediately -- even mid-
  //    edit (`focused`) or with an unresolved error still up -- because the alternative is a
  //    field that silently disagrees with the record it is bound to.
  //  - `value` is unchanged and only the format (or nothing) did: reformatting stale text here
  //    must not fight active typing, nor clobber a still-unresolved error out from under the
  //    user -- that softer case stays gated on `focused`/`error`, and is handled below by
  //    whichever of the two effects actually applies (this one when there is no error, the next
  //    one when there is).
  $effect(() => {
    const v = value;
    const fmt = $dateFormat;
    if (v !== lastSeenValue) {
      lastSeenValue = v;
      text = fmtDate(v, fmt);
      markError(null);
      return;
    }
    if (focused) return;
    if (untrack(() => error) !== null) return;
    const canonical = fmtDate(v, fmt);
    if (text !== canonical) text = canonical;
  });

  // The format-change-only case for an already-showing error: if the currently typed text would
  // now commit cleanly under the new format, actually commit it (not just clear the message --
  // the field would otherwise show a plain, unremarked-on date that was never saved to `value`).
  // Otherwise refresh the message text, which embeds a formatted date (the invalid-input example,
  // or the min/max of an out-of-range one) and so is itself format-dependent. `text`/`error` are
  // read through `untrack` so typing alone never re-runs this.
  $effect(() => {
    const fmt = $dateFormat;
    if (untrack(() => error) === null) return;
    const msg = untrack(() => messageFor(text, fmt));
    if (msg === null) untrack(() => commit());
    else markError(msg);
  });

  function openPicker() {
    const chosen = value ? new Date(`${value}T12:00:00Z`) : new Date();
    view = new Date(Date.UTC(chosen.getUTCFullYear(), chosen.getUTCMonth(), 1));
    pickerOpen = !pickerOpen;
  }

  function isoDate(d: Date): string { return d.toISOString().slice(0, 10); }
  function pick(d: Date) {
    const picked = isoDate(d);
    const range = rangeMessage(picked, $dateFormat);
    // An out-of-range PICK still goes into the field: the error then describes what is actually
    // on screen, rather than referring to a value the user never typed. `value`/`lastSeenValue`
    // stay at whatever was last valid -- only the display changes -- exactly like an out-of-range
    // TYPED date, where `commit()` likewise leaves `value` alone and shows the error next to what
    // was typed.
    text = fmtDate(picked, $dateFormat);
    markError(range);
    if (!range) { value = picked; lastSeenValue = picked; }
    pickerOpen = false;
  }

  function moveMonth(delta: number) { view = new Date(Date.UTC(view.getUTCFullYear(), view.getUTCMonth() + delta, 1)); }
</script>

<div class="date-input">
  <input {id} bind:this={field} type="text" inputmode="numeric" autocomplete="off" {required} aria-label={label}
         placeholder={datePlaceholder($dateFormat, $locale)} bind:value={text}
         aria-invalid={!!error} aria-describedby={error ? `${id}-error` : undefined}
         onfocus={() => (focused = true)} onblur={() => { focused = false; commit(); }} onchange={commit} />
  <button type="button" class="ghost" aria-label={$t('date.pick')} aria-expanded={pickerOpen} onclick={openPicker}><Icon name="calendar" size={18} /></button>
  {#if pickerOpen}
    <div class="calendar" role="dialog" aria-label={$t('date.pick')}>
      <div class="calendar-head"><button type="button" class="ghost" aria-label={$t('date.previous-month')} onclick={() => moveMonth(-1)}>‹</button><strong>{monthLabel}</strong><button type="button" class="ghost" aria-label={$t('date.next-month')} onclick={() => moveMonth(1)}>›</button></div>
      <div class="calendar-grid">{#each weekdays as day}<span class="weekday">{day}</span>{/each}
        {#each calendarDays as day}
          <button type="button" class:outside={day.getUTCMonth() !== view.getUTCMonth()} class:selected={isoDate(day) === value} onclick={() => pick(day)}>{day.getUTCDate()}</button>
        {/each}
      </div>
    </div>
  {/if}
</div>
{#if error}
  <span id={`${id}-error`} class="error" aria-live="polite">{error}</span>
{/if}

<style>
  .date-input { display: flex; gap: var(--space-1); align-items: center; position: relative; }
  .date-input input[type='text'] { flex: 1; min-width: 0; }
  .calendar { position: absolute; z-index: 20; top: calc(100% + 4px); right: 0; width: min(20rem, 90vw); padding: var(--space-2); border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface); box-shadow: var(--shadow); }
  .calendar-head { display: grid; grid-template-columns: auto 1fr auto; align-items: center; text-align: center; }
  .calendar-grid { display: grid; grid-template-columns: repeat(7, 1fr); gap: 2px; }
  .calendar-grid button { min-width: 0; padding: var(--space-2) var(--space-1); }
  .weekday { text-align: center; color: var(--muted); font-size: var(--text-xs); padding: var(--space-1) 0; }
  .outside { opacity: .45; }
  .selected { outline: 2px solid var(--accent); }
</style>
