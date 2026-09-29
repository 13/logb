<script lang="ts">
  import { tick } from 'svelte';
  import { t } from '../i18n';
  import { customTypes, typeIcon } from './type-registry';
  import Icon from './Icon.svelte';
  import type { MemObject } from './types';

  let { objects, onpick }: { objects: MemObject[]; onpick: (o: MemObject) => void } = $props();

  let dialog = $state<HTMLDialogElement | null>(null);
  let search = $state<HTMLInputElement | null>(null);
  let query = $state('');
  const fold = (s: string) => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
  const shown = $derived(query.trim() === '' ? objects : objects.filter((o) => fold(o.name).includes(fold(query.trim()))));

  /** The list exists only while the dialog is open: a closed dialog's buttons are hidden, but
   *  still in the page for anything that looks the object names up by text. */
  let isOpen = $state(false);

  async function open() {
    query = '';
    isOpen = true;
    await tick();
    dialog?.showModal();
    // A native dialog focuses its first focusable element, which is the search box, but only
    // after it opens; asking for it explicitly keeps that true if the order ever changes.
    search?.focus();
  }
  /** Chrome's first Escape in a `type=search` field only clears the typed text and never reaches
   *  the dialog's own close handling; closing explicitly makes one press always close it. */
  function closeOnEscape(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); dialog?.close(); }
  }
  function pick(o: MemObject) { dialog?.close(); onpick(o); }
  const focus = 'focus-visible:outline-2 focus-visible:outline-solid focus-visible:outline-offset-2 focus-visible:outline-ring';
</script>

<!-- A native dialog, not a bits-ui one: this is on the dashboard, the screen every start draws
     first, and bits-ui's shared core would ride along into that chunk. -->
<button data-slot="dash-fab" onclick={open}
        class={`fab h-12 cursor-pointer rounded-full bg-primary px-5 text-base font-semibold text-primary-foreground shadow-lg ${focus}`}>+ {$t('dash.log')}</button>

<dialog bind:this={dialog} closedby="any" aria-labelledby="log-picker-title" onclose={() => (isOpen = false)}
        class="m-auto w-[min(92vw,28rem)] rounded-xl border border-border bg-popover p-0 text-popover-foreground shadow-xl backdrop:bg-black/45">
  {#if isOpen}
  <div class="flex flex-col gap-3 p-4">
    <h2 id="log-picker-title" class="m-0 text-lg font-semibold">{$t('dash.pick-title')}</h2>
    <input bind:this={search} bind:value={query} type="search" data-slot="dash-search"
           onkeydown={closeOnEscape} aria-label={$t('dash.pick-search')} placeholder={$t('dash.pick-search')}
           class={`h-11 rounded-md border border-input bg-card px-3 text-base text-foreground placeholder:text-muted-foreground ${focus}`} />
    <div class="flex max-h-[50vh] flex-col overflow-y-auto">
      {#each shown as o (o.id)}
        <button data-slot="dash-pick" onclick={() => pick(o)}
                class={`flex min-h-11 cursor-pointer items-center gap-3 rounded-md px-2 text-left text-foreground hover:bg-accent ${focus} focus-visible:outline-offset-[-2px]`}>
          <Icon name={typeIcon(o.type, $customTypes)} size={18} />
          <span class="truncate">{o.name}</span>
        </button>
      {:else}
        <p class="m-0 py-2 text-sm text-muted-foreground">{$t('dash.pick-none')}</p>
      {/each}
    </div>
    <div class="flex justify-end border-t border-border pt-3">
      <button data-slot="dash-action" onclick={() => dialog?.close()}
              class={`min-h-11 cursor-pointer rounded-md border border-border bg-card px-3 text-sm font-medium text-foreground ${focus}`}>{$t('nav.cancel')}</button>
    </div>
  </div>
  {/if}
</dialog>
