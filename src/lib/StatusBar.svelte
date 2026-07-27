<script>
  let { message = "" } = $props();
</script>

{#if message}
  <footer class="status-bar">
    <span class="status-dot"></span>
    <span class="status-text">{message}</span>
  </footer>
{/if}

