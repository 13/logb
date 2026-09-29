<script lang="ts">
  import { go, path } from './router';
  import { t } from '../i18n';
  import Icon from './Icon.svelte';
  import AppFooter from './AppFooter.svelte';
  import Logo from './Logo.svelte';
  import { DESTINATIONS, activeDestination } from './nav';

  const active = $derived(activeDestination($path));
</script>

<!-- One nav, two presentations. Which one you get is decided by Tailwind's `desk:`/`max-desk:`
     variants, which are media queries (`width >= 900px` / `width < 900px`), not by a matchMedia
     read here: a breakpoint measured in JavaScript is wrong on first paint and wrong again on
     every resize, and this element is on screen for all of both.

     Mobile: a bottom tab bar. The top of a phone screen is the one place a thumb cannot reach,
     which is exactly where every navigation control in this app used to live. Desktop: a sidebar.
     Every layout property is set under one variant or the other, never bare, so that no width
     (fractional ones under OS display scaling included) leaves the fixed nav without an inset. -->
<nav
  class="appnav fixed z-[7] bg-card
         max-desk:inset-x-0 max-desk:bottom-0 max-desk:border-t max-desk:border-border max-desk:pb-[env(safe-area-inset-bottom)]
         desk:inset-y-0 desk:left-0 desk:flex desk:w-60 desk:flex-col desk:gap-4 desk:border-r desk:border-border desk:px-3 desk:py-4"
  aria-label={$t('nav.primary')}
>
  <!-- Desktop only: the phone's top bar is already full with the back button, the title and the
       outbox chips. -->
  <span class="hidden items-center gap-2 px-2 text-lg font-bold tracking-tight desk:flex"><Logo size={28} decorative inline />LogB</span>
  <ul class="m-0 flex list-none p-0 desk:flex-col desk:gap-1">
    {#each DESTINATIONS as d (d.id)}
      <li class="flex-1 desk:flex-none">
        <button
          data-slot="nav-item"
          class={[
            'flex w-full cursor-pointer items-center transition-colors',
            'max-desk:min-h-14 max-desk:flex-col max-desk:justify-center max-desk:gap-0.5 max-desk:p-1 max-desk:text-xs',
            'desk:min-h-11 desk:gap-3 desk:rounded-md desk:px-3 desk:text-sm',
            active === d.id
              ? 'font-semibold text-brand-ink desk:bg-primary/10'
              : 'text-muted-foreground hover:text-foreground desk:hover:bg-accent',
          ]}
          aria-current={active === d.id ? 'page' : undefined}
          onclick={() => go(d.path)}
        >
          <Icon name={d.icon} />
          <span>{$t(d.label)}</span>
        </button>
      </li>
    {/each}
  </ul>
  <!-- No footer on a phone: the version lives in Settings > About there. -->
  <div class="hidden desk:mt-auto desk:block"><AppFooter /></div>
</nav>
