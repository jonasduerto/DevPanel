<script>
  import { tick } from "svelte";

  let {
    open = $bindable(false),
    label = "More actions",
    title = label,
    align = "right",
    gap = 4,
    triggerClass = "",
    onopen = () => {},
    children,
    trigger = undefined,
  } = $props();

  /** @type {HTMLDivElement | undefined} */
  let wrap = $state();
  /** @type {HTMLButtonElement | undefined} */
  let triggerEl = $state();
  /** @type {HTMLDivElement | undefined} */
  let popupEl = $state();

  let top = $state(0);
  let left = $state(0);
  let positioned = $state(false);

  $effect(() => {
    if (!open) return;
    positioned = false;
    onopen();
    const reposition = () => position();
    tick().then(reposition);
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
    return () => {
      window.removeEventListener("resize", reposition);
      window.removeEventListener("scroll", reposition, true);
    };
  });

  function position() {
    if (!triggerEl || !popupEl) return;
    const rect = triggerEl.getBoundingClientRect();
    const popupWidth = popupEl.offsetWidth || 170;
    const popupHeight = popupEl.offsetHeight;
    let nextTop = rect.bottom + gap;
    let nextLeft = align === "left" ? rect.left : rect.right - popupWidth;
    if (nextTop + popupHeight > window.innerHeight - 8) {
      nextTop = Math.max(8, rect.top - popupHeight - gap);
    }
    if (nextLeft < 8) nextLeft = 8;
    if (nextLeft + popupWidth > window.innerWidth - 8) nextLeft = window.innerWidth - 8 - popupWidth;
    top = nextTop;
    left = nextLeft;
    positioned = true;
  }

  /** @param {KeyboardEvent} event */
  function handleKeydown(event) {
    if (event.key === "Escape") open = false;
  }
</script>

<svelte:window
  onclick={(event) => {
    if (event.target instanceof Node && !wrap?.contains(event.target)) open = false;
  }}
  onkeydown={handleKeydown}
/>

<div class="popup-wrap" bind:this={wrap}>
  <button
    bind:this={triggerEl}
    class="menu-trigger {triggerClass}"
    onclick={() => { open = !open; }}
    aria-label={label}
    {title}
    aria-haspopup="menu"
    aria-expanded={open}
  >
    {#if trigger}{@render trigger?.()}{:else}•••{/if}
  </button>

  {#if open}
    <div
      bind:this={popupEl}
      class="popup"
      role="menu"
      tabindex="-1"
      style:top={`${top}px`}
      style:left={`${left}px`}
      style:visibility={positioned ? "visible" : "hidden"}
    >
      {@render children?.()}
    </div>
  {/if}
</div>
