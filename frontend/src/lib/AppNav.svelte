<script lang="ts">
  import { go, path } from './router';
  import { t } from '../i18n';
  import Icon from './Icon.svelte';
  import AppFooter from './AppFooter.svelte';
  import { DESTINATIONS, activeDestination } from './nav';

  const active = $derived(activeDestination($path));
</script>

<!-- One nav, two presentations. Which one you get is decided in app.css by a media query, not
     here by a matchMedia read: a breakpoint measured in JavaScript is wrong on first paint and
     wrong again on every resize, and this element is on screen for all of both. -->
<nav class="appnav" aria-label={$t('nav.primary')}>
  <span class="wordmark">LogB</span>
  <ul>
    {#each DESTINATIONS as d (d.id)}
      <li>
        <button
          class="dest"
          class:active={active === d.id}
          aria-current={active === d.id ? 'page' : undefined}
          onclick={() => go(d.path)}
        >
          <Icon name={d.icon} />
          <span>{$t(d.label)}</span>
        </button>
      </li>
    {/each}
  </ul>
  <div class="navfoot"><AppFooter /></div>
</nav>
