<script lang="ts">
</script>

<!-- The same file the favicon and the PWA manifest use, so there is one drawing to change.
     `alt` rather than a decorative empty string: on Login this is the only thing naming the
     application, since its heading is just "Sign in" (Setup's heading already says "Welcome
     to LogB", but this component doesn't know which screen it's on). -->
<img class="logo" src="/icon.svg" alt="LogB" width="64" height="64" />

<style>
  .logo {
    display: block;
    margin: 0 auto 12px;
  }
</style>
