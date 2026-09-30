<script lang="ts">
  import { controlClass } from '$lib/components/ui/field/classes.js';
  import { t } from '../i18n';

  /**
   * A password field with a show/hide toggle. Plain class strings and inline icons, nothing from
   * the component library: Login and Setup use it, and they are the first screen a fresh install
   * shows. The caller draws the label (`for={id}`).
   */
  let { id, value = $bindable(''), autocomplete, required = false, minlength, describedby }: {
    id: string; value?: string; autocomplete: 'current-password' | 'new-password' | 'off';
    required?: boolean; minlength?: number; describedby?: string;
  } = $props();
  let shown = $state(false);
</script>

<div data-slot="password" class="relative min-w-0">
  <input {id} data-slot="password-input" type={shown ? 'text' : 'password'} bind:value {autocomplete} {required} {minlength}
         autocapitalize="off" spellcheck="false" aria-describedby={describedby} class={`${controlClass} pr-12`} />
  <!-- Named by its text, not `aria-label`: `getByLabel(/Password|Passwort/)` must find only the
       field, and "Passwort anzeigen" would match it. The name stays; `aria-pressed` says whether
       the password is shown. 48 px square, the field's own height. -->
  <button type="button" data-slot="password-toggle" aria-pressed={shown} aria-controls={id} onclick={() => (shown = !shown)}
          class="absolute inset-y-0 right-0 grid w-12 cursor-pointer place-items-center rounded-r-lg text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-ring">
    <span class="sr-only">{$t('login.show-password')}</span>
    <svg aria-hidden="true" viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      {#if shown}
        <path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49" />
        <path d="M14.084 14.158a3 3 0 0 1-4.242-4.242" />
        <path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143" />
        <path d="m2 2 20 20" />
      {:else}
        <path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" />
        <circle cx="12" cy="12" r="3" />
      {/if}
    </svg>
  </button>
</div>
