<script lang="ts">
  import { onDestroy } from 'svelte';
  import { clearToast, toastMessage } from './toast';

  // A message belongs to the page that showed it: leaving drops it, so the next page never
  // announces "Saved" for something it did not do.
  onDestroy(clearToast);
</script>

<!-- Mounted only by route components, one per page that saves or reports (never by a shared
     component, so a page never has two live regions saying the same thing).
     Always in the page, empty until there is something to say: a live region inserted together
     with its text is often not announced. Above the tab bar on a phone, bottom centre of the
     content pane on a desktop. Foreground on background, inverted: the pair is already tested. -->
<div role="status" aria-live="polite" aria-atomic="true" data-testid="toast"
     class="pointer-events-none fixed inset-x-0 z-30 flex justify-center px-4 max-desk:bottom-[calc(var(--navbar)+env(safe-area-inset-bottom)+0.75rem)] desk:bottom-6 desk:left-60">
  {#if $toastMessage}
    {#key $toastMessage.id}
      <p class="m-0 max-w-md rounded-2xl bg-foreground px-4 py-2 text-center text-sm font-medium text-background shadow-lg">{$toastMessage.text}</p>
    {/key}
  {/if}
</div>
