<script lang="ts">
  import { flushSync, type Snippet } from 'svelte';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import { t } from '../i18n';

  /**
   * The optional half of a form. Closed for a new entry; the form sets `open` when what it edits
   * uses any of these fields. Closed means `hidden`, not unmounted: what was typed survives a
   * close, and the form still submits it.
   *
   * A closed section must not swallow a validation error. The browser checks every field on
   * submit, hidden or not, and cannot focus one it cannot show: an invalid date in here would
   * block the save with nothing on screen. So an `invalid` event from inside opens the section
   * at once (`flushSync`), before the browser looks for the field to focus.
   */
  let { open = $bindable(false), id = 'more-details', children }: { open?: boolean; id?: string; children: Snippet } = $props();
</script>

<div data-slot="more-details" class="flex flex-col gap-4 border-t border-border pt-2" oninvalidcapture={() => { if (!open) { open = true; flushSync(); } }}>
  <button type="button" data-slot="more-details-toggle" aria-expanded={open} aria-controls={id}
          class="flex min-h-11 w-full cursor-pointer items-center justify-between gap-2 text-left text-sm font-semibold text-foreground hover:text-brand-ink focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring"
          onclick={() => (open = !open)}>
    {$t('form.more-details')}
    <ChevronDown aria-hidden="true" class={['size-4 text-muted-foreground transition-transform motion-reduce:transition-none', open && 'rotate-180']} />
  </button>
  <div {id} hidden={!open} class="flex flex-col gap-5">{@render children()}</div>
</div>
