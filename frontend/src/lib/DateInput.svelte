<script lang="ts">
  import { dateTimeFormat } from './intl-cache';
  import { tick, untrack } from 'svelte';
  import { t, locale } from '../i18n';
  import { dateFormat } from '../stores/date-format';
  import { settings } from '../stores/settings';
  import { datePlaceholder, fmtDate, parseDate, type DateFormat } from './format';
  import Icon from './Icon.svelte';
  import { cn } from '$lib/utils.js';
  import { controlClass } from '$lib/components/ui/field/classes.js';
  import { getFieldContext } from '$lib/components/ui/field/context.js';

  let { id, value = $bindable(''), min, max, required = false, label }:
    { id: string; value?: string; min?: string; max?: string; required?: boolean; label?: string } = $props();

  /** The `Field` around this input, if any: its id, hint and error describe the field too. The
   *  input's own format error, while shown, is what describes it (one id: see 26-date-format). */
  const fieldCtx = getFieldContext();
  const inputId = $derived(fieldCtx?.id ?? id);

  let text = $state(fmtDate(value, $dateFormat));
  /** The showing error message, or `null` when the field is valid. A string (not a boolean),
   *  because there are two different messages to show: an unparsable date, or a parsed-but-out-
   *  of-range one. */
  let error = $state<string | null>(null);
  let focused = $state(false);
  let field: HTMLInputElement;
  let pickerOpen = $state(false);
  let view = $state(new Date());
  let container: HTMLDivElement;
  let trigger: HTMLButtonElement;
  let focusedDay = $state('');

  function closePicker() { pickerOpen = false; trigger?.focus(); }
  function outside(event: PointerEvent) {
    if (pickerOpen && event.target instanceof Node && !container.contains(event.target)) pickerOpen = false;
  }
  async function focusDay(date: Date) {
    focusedDay = isoDate(date);
    view = new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), 1));
    await tick();
    container.querySelector<HTMLButtonElement>(`[data-date="${focusedDay}"]`)?.focus();
  }
  function calendarKey(event: KeyboardEvent, date: Date) {
    const next = new Date(date);
    const steps: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 };
    if (event.key in steps) next.setUTCDate(next.getUTCDate() + steps[event.key]);
    else if (event.key === 'Home') next.setUTCDate(next.getUTCDate() - (next.getUTCDay() - (weekStartsMonday ? 1 : 0) + 7) % 7);
    else if (event.key === 'End') next.setUTCDate(next.getUTCDate() + 6 - (next.getUTCDay() - (weekStartsMonday ? 1 : 0) + 7) % 7);
    else if (event.key === 'PageUp' || event.key === 'PageDown') {
      const d = next.getUTCDate();
      next.setUTCDate(1);
      next.setUTCMonth(next.getUTCMonth() + (event.key === 'PageUp' ? -1 : 1));
      next.setUTCDate(Math.min(d, new Date(Date.UTC(next.getUTCFullYear(), next.getUTCMonth() + 1, 0)).getUTCDate()));
    } else return;
    event.preventDefault();
    void focusDay(next);
  }

  const weekStartsMonday = $derived($settings.firstDayOfWeek === 'monday' || ($settings.firstDayOfWeek === 'locale' && $locale === 'de'));
  const weekdays = $derived(Array.from({ length: 7 }, (_, i) => {
    const offset = (i + (weekStartsMonday ? 1 : 0)) % 7;
    return dateTimeFormat($locale, { weekday: 'short', timeZone: 'UTC' }).format(new Date(Date.UTC(2026, 7, 2 + offset)));
  }));
  const calendarDays = $derived.by(() => {
    const y = view.getUTCFullYear(), m = view.getUTCMonth();
    const first = new Date(Date.UTC(y, m, 1));
    const lead = (first.getUTCDay() - (weekStartsMonday ? 1 : 0) + 7) % 7;
    return Array.from({ length: 42 }, (_, i) => new Date(Date.UTC(y, m, i - lead + 1)));
  });
  const monthLabel = $derived(dateTimeFormat($locale, { month: 'long', year: 'numeric', timeZone: 'UTC' }).format(view));

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
    if (pickerOpen) { closePicker(); return; }
    const now = new Date();
    const chosen = value ? new Date(`${value}T12:00:00Z`) : new Date(Date.UTC(now.getFullYear(), now.getMonth(), now.getDate()));
    pickerOpen = true;
    void focusDay(chosen);
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
    closePicker();
  }

  function moveMonth(delta: number) {
    view = new Date(Date.UTC(view.getUTCFullYear(), view.getUTCMonth() + delta, 1));
    focusedDay = isoDate(view);
  }
</script>

<svelte:window onpointerdown={outside} onkeydown={(e) => { if (pickerOpen && e.key === 'Escape') { e.preventDefault(); closePicker(); } }} />
<div data-slot="date-input" class="relative min-w-0" bind:this={container} onfocusout={(e) => { if (e.relatedTarget instanceof Node && !container.contains(e.relatedTarget)) pickerOpen = false; }}>
  <input id={inputId} bind:this={field} data-slot="date-text" type="text" inputmode="numeric" autocomplete="off" {required} aria-label={label}
         placeholder={datePlaceholder($dateFormat, $locale)} bind:value={text}
         aria-invalid={!!error || fieldCtx?.invalid || undefined}
         aria-describedby={error ? `${inputId}-error` : fieldCtx?.describedBy}
         class={cn(controlClass, 'pr-12 tabular-nums')}
         onfocus={() => (focused = true)} onblur={() => { focused = false; commit(); }} onchange={commit} />
  <!-- Inside the field's box, at its right edge: beside it, it squeezed the text to "MM/DD/YYY". -->
  <button bind:this={trigger} type="button" data-slot="date-trigger" aria-label={$t('date.pick')} aria-haspopup="dialog" aria-controls={`${inputId}-calendar`} aria-expanded={pickerOpen} onclick={openPicker}
          class="absolute top-1/2 right-0.5 grid size-11 -translate-y-1/2 cursor-pointer place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring"><Icon name="calendar" size={18} /></button>
  {#if pickerOpen}
    <div id={`${inputId}-calendar`} role="dialog" aria-label={$t('date.pick')}
         class="absolute top-[calc(100%+4px)] right-0 z-20 w-[min(20rem,90vw)] rounded-lg border border-border bg-popover p-2 text-popover-foreground shadow-md">
      <div class="grid grid-cols-[auto_1fr_auto] items-center text-center">
        <button type="button" data-slot="calendar-nav" class="grid size-11 cursor-pointer place-items-center rounded-md text-lg hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring" aria-label={$t('date.previous-month')} onclick={() => moveMonth(-1)}>‹</button>
        <strong class="text-sm font-semibold">{monthLabel}</strong>
        <button type="button" data-slot="calendar-nav" class="grid size-11 cursor-pointer place-items-center rounded-md text-lg hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring" aria-label={$t('date.next-month')} onclick={() => moveMonth(1)}>›</button>
      </div>
      <div class="grid grid-cols-7 gap-0.5">
        {#each weekdays as day}<span data-testid="calendar-weekday" class="py-1 text-center text-xs text-muted-foreground">{day}</span>{/each}
        {#each calendarDays as day}
          <button type="button" data-slot="calendar-day" data-date={isoDate(day)} tabindex={isoDate(day) === focusedDay ? 0 : -1}
            aria-label={dateTimeFormat($locale, { dateStyle: 'full', timeZone: 'UTC' }).format(day)}
            aria-pressed={isoDate(day) === value} aria-disabled={!!rangeMessage(isoDate(day), $dateFormat)}
            class={[
              'min-h-11 min-w-0 cursor-pointer rounded-md text-sm tabular-nums not-aria-pressed:hover:bg-accent focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring aria-pressed:bg-primary aria-pressed:font-semibold aria-pressed:text-primary-foreground aria-disabled:cursor-not-allowed aria-disabled:opacity-40',
              day.getUTCMonth() !== view.getUTCMonth() && 'text-muted-foreground',
            ]}
            onkeydown={(e) => calendarKey(e, day)} onclick={() => { if (!rangeMessage(isoDate(day), $dateFormat)) pick(day); }}>{day.getUTCDate()}</button>
        {/each}
      </div>
    </div>
  {/if}
</div>
{#if error}
  <span id={`${inputId}-error`} class="text-sm font-medium text-destructive" aria-live="polite">{error}</span>
{/if}
