<script>
  // @ts-nocheck
  import { onMount } from "svelte";

  const categories = [];

  let activeCategory = $state("");
</script>

<div class="tools-view">
  <div class="tools-sidebar">
    {#each categories as cat (cat.id)}
      <button
        class="cat-btn"
        class:active={activeCategory === cat.id}
        onclick={() => (activeCategory = cat.id)}
      >
        <cat.icon size={14} />
        <span>{cat.label}</span>
      </button>
    {/each}
  </div>

  <div class="tools-content">
    <div class="placeholder-content">
      <p>Cross-cutting tools are being rebuilt here (Xdebug, SSL &amp; Hosts, DB GUI launcher, log viewer, port inspector, Mailpit).</p>
    </div>
  </div>
</div>
