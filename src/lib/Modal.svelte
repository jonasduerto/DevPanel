<script>
  let {
    children,
    onClose,
    ariaLabel = "Dialog",
    width = "85%",
    maxHeight = undefined,
    padding = "16px",
    overlayPadding = "0px",
    scrollable = false,
  } = $props();

  /** @param {KeyboardEvent} event */
  function handleKeydown(event) {
    if (event.key === "Escape") onClose?.();
  }

  /** @param {KeyboardEvent} event */
  function handleDialogKeydown(event) {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    onClose?.();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="overlay"
  role="presentation"
  onclick={onClose}
  onkeydown={() => {}}
  style:padding={overlayPadding}
>
  <div
    class="dialog"
    class:scrollable
    role="dialog"
    aria-modal="true"
    aria-label={ariaLabel}
    tabindex="-1"
    style:width
    style:max-height={maxHeight}
    style:padding
    onclick={(event) => event.stopPropagation()}
    onkeydown={handleDialogKeydown}
  >
    {@render children?.()}
  </div>
</div>

