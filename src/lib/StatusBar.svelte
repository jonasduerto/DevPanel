<script>
  import { invoke } from "@tauri-apps/api/core";
  import { onMount, onDestroy } from "svelte";

  let { message = "" } = $props();

  /** @type {Array<{id: string, name: string}>} */
  let onlineServices = $state([]);
  let runningSites = $state(0);
  /** @type {ReturnType<typeof setInterval> | null} */
  let pollTimer = null;

  async function refreshRuntimeStatus() {
    try {
      const [services, statuses, workspaces] = await Promise.all([
        invoke("get_services"),
        invoke("get_service_statuses"),
        invoke("list_workspaces"),
      ]);
      const stateById = new Map(statuses);
      runningSites = workspaces.filter((/** @type {any} */ site) => site.running).length;
      onlineServices = runningSites
        ? services.filter((/** @type {any} */ service) => stateById.get(service.id) === "Running")
        : [];
    } catch {
      // Runtime status is informative; it must never block the app shell.
    }
  }

  onMount(() => {
    refreshRuntimeStatus();
    pollTimer = setInterval(refreshRuntimeStatus, 5000);
  });

  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });
</script>

<footer class="status-bar">
  <div class="status-primary">
    {#if message}
      <span class="status-dot"></span>
      <span class="status-text">{message}</span>
    {:else if runningSites}
      <span class="status-dot"></span>
      <span class="status-text">{runningSites} active site{runningSites === 1 ? "" : "s"}</span>
    {:else}
      <span class="status-text">No active sites</span>
    {/if}
  </div>
  {#if onlineServices.length}
    <div class="runtime-status" aria-label="Online services">
      <span>Online:</span>
      {#each onlineServices as service (service.id)}
        <span class="runtime-service"><i></i>{service.name}</span>
      {/each}
    </div>
  {/if}
</footer>

