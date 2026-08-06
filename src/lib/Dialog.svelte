<script>
  let {
    title = undefined,
    ariaLabel = title ?? "Dialog",
    onClose = undefined,
    width = "85%",
    maxHeight = undefined,
    padding = "16px",
    overlayPadding = "0px",
    scrollable = false,
    header = undefined,
    children = undefined,
    footer = undefined,
  } = $props();

  /** @param {KeyboardEvent} event */
  function handleKeydown(event) {
    if (event.key === "Escape") onClose?.();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="dialog-overlay"
  role="presentation"
  onclick={onClose}
  style:padding={overlayPadding}
>
  <div
    class="dialog-box"
    class:scrollable
    role="dialog"
    aria-modal="true"
    aria-label={ariaLabel}
    tabindex="-1"
    style:width
    style:max-height={maxHeight}
    onclick={(event) => event.stopPropagation()}
    onkeydown={() => {}}
  >
    {#if header}
      <div class="dialog-header">
        {@render header()}
      </div>
    {:else if title}
      <div class="dialog-header">
        <span class="dialog-title">{title}</span>
        {#if onClose}
          <button class="btn-close-x" onclick={onClose} aria-label="Close" title="Close">✕</button>
        {/if}
      </div>
    {/if}

    <div class="dialog-body" style:padding>
      {@render children?.()}
    </div>

    {#if footer}
      <div class="dialog-footer">
        {@render footer()}
      </div>
    {/if}
  </div>
</div>
