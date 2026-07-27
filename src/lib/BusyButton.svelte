<script>
  import Loader from "#lib/Loader.svelte";
  let { busy = false, busyLabel = "Working", children, onclick, class: className = "", ...rest } = $props();
</script>

<button class={className} disabled={busy || rest.disabled} {onclick} {...rest}>
  {#if busy}<Loader small />{/if}
  {busy ? `${busyLabel}…` : ""}{#if !busy}{@render children?.()}{/if}
</button>
