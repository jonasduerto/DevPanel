<script>
  let { busy = false, operation = "" } = $props();
</script>

{#if busy}
  <div class="loading-bar" role="status" aria-label={operation || "Loading"}></div>
{/if}

