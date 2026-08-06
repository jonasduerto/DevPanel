<script>
  import Dialog from "./Dialog.svelte";

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

  // Removing only the site registration/files while retaining the database is
  // recoverable. Keep the explicit name confirmation for permanent deletion.
  let canConfirm = $derived((showKeepDatabase && keepDatabase) || !requireTextInput || typedText === requireTextInput);

  function handleConfirm() {
    if (!canConfirm) return;
    onConfirm?.(showKeepDatabase ? { keepDatabase } : undefined);
  }
</script>

<Dialog
  title="Confirmation"
  onClose={onCancel}
  ariaLabel="Confirmation"
  width="min(440px, calc(100vw - 32px))"
  padding="20px"
>
  <div class="confirm-dialog">
    <p>{message}</p>

    {#if requireTextInput && !(showKeepDatabase && keepDatabase)}
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
</Dialog>

