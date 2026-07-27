<script>
  import { Check, X, TriangleAlert, Info } from "@lucide/svelte";

  let { toasts = [] } = $props();
</script>

{#if toasts.length}
  <div class="toast-container">
    {#each toasts as toast (toast.id)}
      <div class="toast {toast.type}">
        <span class="toast-icon">
          {#if toast.type === "success"}
            <Check size={14} />
          {:else if toast.type === "error"}
            <X size={14} />
          {:else if toast.type === "warning"}
            <TriangleAlert size={14} />
          {:else}
            <Info size={14} />
          {/if}
        </span>
        <span class="toast-msg">{toast.message}</span>
      </div>
    {/each}
  </div>
{/if}

