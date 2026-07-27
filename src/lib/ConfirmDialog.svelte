<script>
  import Modal from "./Modal.svelte";

  let {
    message,
    confirmLabel = "Confirm",
    onConfirm,
    onCancel,
    requireTextInput = "",
    showKeepDatabase = false,
  } = $props();

  let typedText = $state("");
  let keepDatabase = $state(false);

  let canConfirm = $derived(!requireTextInput || typedText === requireTextInput);

  function handleConfirm() {
    if (!canConfirm) return;
    onConfirm?.(showKeepDatabase ? { keepDatabase } : undefined);
  }
</script>

<Modal onClose={onCancel} ariaLabel="Confirmation">
  <div class="confirm-dialog">
    <p>{message}</p>

    {#if requireTextInput}
      <div class="confirm-type">
        <label for="confirm-text">Type <strong>{requireTextInput}</strong> to confirm:</label>
        <input
          id="confirm-text"
          bind:value={typedText}
          placeholder={requireTextInput}
          onkeydown={(e) => e.key === "Enter" && canConfirm && handleConfirm()}
        />
      </div>
    {/if}

    {#if showKeepDatabase}
      <label class="keep-db">
        <input type="checkbox" bind:checked={keepDatabase} />
        Keep database (remove only site files and virtual host)
      </label>
    {/if}

    <div class="actions">
      <button class="btn-cancel" onclick={onCancel}>Cancel</button>
      <button class="btn-confirm" onclick={handleConfirm} disabled={!canConfirm}>
        {confirmLabel}
      </button>
    </div>
  </div>
</Modal>

