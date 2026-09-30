<script lang="ts">
  import { Button } from '$lib/components/ui/button/index.js';
  import { errorClass } from '$lib/components/ui/field/classes.js';
  import { t } from '../i18n';

  /**
   * Save and Cancel, stuck to the bottom of the screen: above the tab bar on a phone, at the
   * viewport's edge from 900 px. It is the form's last child, so it sticks while the form runs on
   * below the fold and settles under the last field at the end: Save is on screen when the form
   * opens and never covers a field. `error` is the form's own line (a network failure, a key no
   * field shows), kept next to the button that caused it.
   */
  let { busy = false, error = '', oncancel }: { busy?: boolean; error?: string; oncancel: () => void } = $props();
</script>

<div data-testid="form-actions"
     class="sticky z-[6] -mx-3 desk:mx-0 mt-2 flex flex-col gap-2 border-t border-border bg-background px-3 py-3 max-desk:bottom-[calc(var(--navbar)+1px+env(safe-area-inset-bottom))] desk:bottom-0">
  {#if error}<p role="alert" class={errorClass}>{error}</p>{/if}
  <div class="flex gap-2 desk:justify-end">
    <Button variant="outline" class="min-h-11 flex-1 desk:min-w-28 desk:flex-none" onclick={oncancel}>{$t('nav.cancel')}</Button>
    <Button type="submit" class="min-h-11 flex-1 desk:min-w-28 desk:flex-none" disabled={busy}>{$t('nav.save')}</Button>
  </div>
</div>
