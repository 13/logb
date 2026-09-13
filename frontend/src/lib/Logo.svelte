<script lang="ts">
  import icon from '../../public/icon.svg?raw';

  let { size = 64, decorative = false, inline = false }: {
    size?: number; decorative?: boolean; inline?: boolean;
  } = $props();

  // The mark is sliced out of the same file the favicon and the PWA manifest use, so there is
  // one drawing to change. Its white ink becomes currentColor, so in the app it takes the theme's
  // accent instead of sitting on a teal plate. tests/pwa-icons.test.ts guards the markers.
  const mark = icon
    .slice(icon.indexOf('<!--mark-->'), icon.indexOf('<!--/mark-->'))
    .replaceAll('#ffffff', 'currentColor');
</script>

<!-- Labelled rather than decorative by default: on Login this is the only thing naming the
     application, since its heading is just "Sign in". Next to the sidebar wordmark it would
     only say "LogB" twice, so that caller passes `decorative`. -->
<svg
  class="logo"
  class:inline
  viewBox="8 8 48 48"
  width={size}
  height={size}
  role={decorative ? undefined : 'img'}
  aria-label={decorative ? undefined : 'LogB'}
  aria-hidden={decorative ? 'true' : undefined}
>{@html mark}</svg>

<style>
  .logo {
    display: block;
    margin: 0 auto var(--space-3);
    color: var(--accent);
    flex: none;
  }
  .logo.inline {
    display: inline-block;
    margin: 0;
  }
</style>
