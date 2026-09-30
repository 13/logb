<script lang="ts">
  import { toastMessage } from './toast';
</script>

<!-- Always in the page, empty until there is something to say: a live region inserted together
     with its text is often not announced. Above the tab bar on a phone, bottom centre of the
     content pane on a desktop. Foreground on background, inverted: the pair is already tested. -->
<div role="status" aria-live="polite" aria-atomic="true" data-testid="toast"
     class="pointer-events-none fixed inset-x-0 z-30 flex justify-center px-4 max-desk:bottom-[calc(var(--navbar)+env(safe-area-inset-bottom)+0.75rem)] desk:bottom-6 desk:left-60">
  {#if $toastMessage}
    {#key $toastMessage.id}
      <p class="m-0 max-w-md rounded-full bg-foreground px-4 py-2 text-center text-sm font-medium text-background shadow-lg">{$toastMessage.text}</p>
    {/key}
  {/if}
</div>
