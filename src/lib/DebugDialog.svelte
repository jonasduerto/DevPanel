<script>
  // @ts-nocheck — Tauri IPC payloads are dynamically shaped JSON; typing
  // every intermediate chain here has no runtime payoff.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import Modal from "./Modal.svelte";
  import InlineBanner from "#lib/InlineBanner.svelte";
  import { invokeWith } from "#lib/tauri-utils.svelte.js";

  let { workspace, onClose } = $props();

  let loading = $state(true);
  let error = $state("");
  let context = $state(null);
  let copied = $state(false);
  let loadState = $state({ busy: false, error: "", operation: "" });

  onMount(load);

  async function load() {
    loading = true;
    error = "";
    await invokeWith(loadState, async () => {
      context = await invoke("get_workspace_debug_context", { id: workspace.id });
    }, "Loading diagnostics", { toastError: false });
    error = loadState.error;
    loading = false;
  }

  async function copyJson() {
    if (!context) return;
    await navigator.clipboard.writeText(JSON.stringify(context, null, 2));
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

</script>

<Modal
  onClose={onClose}
  ariaLabel={`Diagnostics for ${workspace.name}`}
  width="100%"
  maxHeight="90%"
  padding="14px"
  overlayPadding="16px"
  scrollable
>
  <div class="debug-dialog">
    <div class="dialog-header">
      <span>Diagnostics — {workspace.name}</span>
      <button class="btn-close-x" onclick={onClose}>✕</button>
    </div>

    {#if loading}
      <div class="hint">Loading diagnostics…</div>
    {:else if error}
      <InlineBanner>{error}</InlineBanner>
    {:else}
      <button class="btn-copy" onclick={copyJson}>
        {copied ? "Copied" : "Copy JSON"}
      </button>
      <pre class="json-preview">{JSON.stringify(context, null, 2)}</pre>
    {/if}
  </div>
</Modal>

