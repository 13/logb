<script lang="ts">
  /** Inline SVG rather than a sprite or a library: there are only a handful of them, they must
   *  take the theme's colour through `currentColor`, and the project has no runtime dependencies. */
  let { name, size = 20 }: {
    name: 'back' | 'settings' | 'search' | 'document' | 'camera' | 'edit' | 'repeat'
      | 'car' | 'e-bike' | 'bike' | 'motorcycle' | 'home' | 'appliance' | 'tool' | 'body' | 'object';
    size?: number;
  } = $props();
</script>

<svg
  width={size} height={size} viewBox="0 0 24 24" fill="none"
  stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"
  aria-hidden="true" focusable="false"
>
  {#if name === 'back'}
    <path d="M19 12H5M12 19l-7-7 7-7" />
  {:else if name === 'settings'}
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
  {:else if name === 'search'}
    <circle cx="11" cy="11" r="7" />
    <path d="M21 21l-4.3-4.3" />
  {:else if name === 'document'}
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <path d="M14 2v6h6" />
  {:else if name === 'camera'}
    <path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z" />
    <circle cx="12" cy="13" r="4" />
  {:else if name === 'edit'}
    <path d="M12 20h9" />
    <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z" />
  {:else if name === 'repeat'}
    <path d="m17 2 4 4-4 4" />
    <path d="M3 11v-1a4 4 0 0 1 4-4h14" />
    <path d="m7 22-4-4 4-4" />
    <path d="M21 13v1a4 4 0 0 1-4 4H3" />
  {:else if name === 'car'}
    <path d="M5 16V11a2 2 0 0 1 2-2h1l1.5-2h5L16 9h1a2 2 0 0 1 2 2v5" />
    <path d="M3 16h18" />
    <circle cx="7.5" cy="17.5" r="1.5" />
    <circle cx="16.5" cy="17.5" r="1.5" />
  {:else if name === 'e-bike'}
    <circle cx="5.5" cy="17.5" r="3.5" />
    <circle cx="18.5" cy="17.5" r="3.5" />
    <path d="M12 17.5V15l-3-2 4-2 2 2h2" />
    <path d="M16.5 1.5 13 6h3l-3.5 4.5" />
  {:else if name === 'bike'}
    <circle cx="5.5" cy="17.5" r="3.5" />
    <circle cx="18.5" cy="17.5" r="3.5" />
    <path d="M12 17.5V14l-3-3 4-3 2 3h2" />
  {:else if name === 'motorcycle'}
    <circle cx="5.5" cy="17.5" r="3.3" />
    <circle cx="18.5" cy="17.5" r="3.3" />
    <path d="M4.5 14.5h4l2.5-2h5l3 4.5" />
    <path d="M17 12.5V9.5h2" />
  {:else if name === 'home'}
    <path d="M4 11 12 4l8 7" />
    <path d="M6 10v9a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1v-9" />
    <path d="M10 20v-5h4v5" />
  {:else if name === 'appliance'}
    <rect x="4" y="3" width="16" height="18" rx="2" />
    <path d="M4 7h16" />
    <circle cx="12" cy="14" r="4.5" />
    <circle cx="12" cy="14" r="1.5" />
  {:else if name === 'tool'}
    <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
  {:else if name === 'body'}
    <circle cx="12" cy="5" r="2.3" />
    <path d="M7 20v-4a5 5 0 0 1 10 0v4" />
  {:else if name === 'object'}
    <path d="M3 8l9-5 9 5-9 5-9-5z" />
    <path d="M3 8v8l9 5 9-5V8" />
    <path d="M12 13v8" />
  {:else}
    {(name satisfies never)}
  {/if}
</svg>
