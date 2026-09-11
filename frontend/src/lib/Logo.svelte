<script lang="ts">
  let { size = 64 }: { size?: number } = $props();
</script>

<!-- The same file the favicon and the PWA manifest use, so there is one drawing to change.
     `alt` rather than a decorative empty string: on Login and Setup this is the only thing
     naming the application, since neither screen shows the app name anywhere else. -->
<img class="logo" src="/icon.svg" alt="logby" width={size} height={size} />

<style>
  .logo {
    display: block;
    margin: 0 auto 12px;
    border-radius: 14px;
  }
</style>
