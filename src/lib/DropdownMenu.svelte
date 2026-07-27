<script>
  let {
    open = $bindable(false),
    label = "More actions",
    title = label,
    align = "right",
    onopen = () => {},
    children,
  } = $props();
  /** @type {HTMLDivElement | undefined} */
  let element;
</script>

<svelte:window onclick={(event) => { if (event.target instanceof Node && !element?.contains(event.target)) open = false; }} />

<div class="menu-wrap" bind:this={element}>
  <button class="menu-trigger" onclick={() => { open = !open; if (open) onopen(); }} aria-label={label} {title}>•••</button>
  {#if open}
    <div class:align-left={align === "left"} class="menu" role="menu" tabindex="-1">
      {@render children?.()}
    </div>
  {/if}
</div>

