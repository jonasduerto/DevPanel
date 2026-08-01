<script>
  // @ts-nocheck
  import { invoke } from "@tauri-apps/api/core";
  import { openPath, openUrl } from "@tauri-apps/plugin-opener";
  import { onMount, onDestroy } from "svelte";
  import ServiceCard from "#lib/ServiceCard.svelte";
  import DropdownMenu from "#lib/DropdownMenu.svelte";
  import InlineBanner from "#lib/InlineBanner.svelte";
  import { invokeWith } from "#lib/tauri-utils.svelte.js";
  import { Activity, Server, Globe, RefreshCw, ExternalLink, ArrowRight, Play, Square, AlertCircle } from "@lucide/svelte";

  let { onNavigateToSites } = $props();

  /** @type {Array<{id: string, name: string, description: string, port?: number, category: string, status: string}>} */
  let services = $state([]);
  /** @type {any[]} */
  let workspaces = $state([]);
  let workspacesLoaded = $state(false);
  let toolState = $state({ busy: false, error: "", operation: "" });
  let pollTimer = null;
  /** @type {Record<string, {configPaths: any, logPaths: any}>} */
  let serviceInfo = $state({});
  /** @type {Record<string, {enabled: boolean, show_on_dashboard: boolean}>} */
  let addonStates = $state({});

  /** Services filtered by addon enabled+visible state */
  let visibleServices = $derived.by(() => {
    return services.filter((s) => {
      const state = addonStates[s.id];
      if (state) {
        return Boolean(state.enabled && state.show_on_dashboard);
      }
      // Try matching by prefix (e.g. php@8.4 -> "php")
      const baseId = s.id.split("@")[0];
      const baseState = addonStates[baseId];
      if (baseState) {
        return Boolean(baseState.enabled && baseState.show_on_dashboard);
      }
      // Do NOT display modules on the dashboard unless they are enabled in Modules state
      return false;
    });
  });

  let runningServicesCount = $derived(visibleServices.filter(s => s.status === "Running").length);
  let totalVisibleCount = $derived(visibleServices.length);
  let runningSitesCount = $derived(workspaces.filter(w => w.running).length);
  let allVisibleRunning = $derived(totalVisibleCount > 0 && runningServicesCount === totalVisibleCount);

  onMount(() => {
    refresh();
    pollTimer = setInterval(refresh, 5000);
  });

  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
  });

  async function refresh() {
    await invokeWith(toolState, async () => {
      const [svcs, statuses, wss, addonSt] = await Promise.all([
        invoke("get_services"),
        invoke("get_service_statuses"),
        invoke("list_workspaces"),
        invoke("get_addon_states"),
      ]);
      const stateById = new Map(statuses);
      services = svcs.map((/** @type {any} */ s) => ({
        ...s,
        status: stateById.get(s.id) ?? "Stopped",
      }));
      workspaces = wss;
      addonStates = addonSt;
      workspacesLoaded = true;
    }, undefined, { toastError: false });
  }

  async function toggle(/** @type {string} */ id, /** @type {string} */ currentStatus) {
    const action = currentStatus === "Running" ? "stop" : "start";
    await invokeWith(toolState, async () => {
      await invoke(`${action}_service`, { id });
    }, `${action === "start" ? "Iniciando" : "Deteniendo"} servicio`);
    await refresh();
  }

  async function toggleSite(/** @type {any} */ site) {
    await invokeWith(toolState, async () => {
      await invoke(site.running ? "stop_workspace" : "start_workspace", { id: site.id });
    }, site.running ? "Stopping site" : "Starting site");
    await refresh();
  }

  async function setAllServices(action) {
    const targets = visibleServices.filter((service) => action === "start" ? service.status !== "Running" : service.status === "Running");
    if (!targets.length) return;
    await invokeWith(toolState, async () => {
      await Promise.all(targets.map((service) => invoke(`${action}_service`, { id: service.id })));
    }, `${action === "start" ? "Starting" : "Stopping"} enabled services`);
    await refresh();
  }

  async function syncHosts() {
    await invokeWith(toolState, async () => {
      const count = await invoke("sync_workspace_hosts");
      toolState.error = `Hosts synchronized for ${count} site${count === 1 ? "" : "s"}.`;
    }, "Synchronizing hosts");
  }

  async function loadServiceInfo(/** @type {string} */ id) {
    if (!serviceInfo[id]) {
      await invokeWith(toolState, async () => {
        const [configPaths, logPaths] = await Promise.all([
          invoke("get_service_config_paths", { id }),
          invoke("get_service_log_paths", { id }),
        ]);
        serviceInfo[id] = { configPaths, logPaths };
      }, "Cargando acciones");
    }
  }

  async function openServicePath(/** @type {string} */ path) {
    await invokeWith(toolState, async () => {
      await openPath(path);
    }, "Abriendo ruta");
  }

  async function runServiceControl(/** @type {string} */ id, /** @type {string} */ command) {
    await invokeWith(toolState, async () => {
      const result = await invoke(command, { id });
      toolState.error = result.success
        ? `${id}: OK` + (result.stdout ? ` — ${result.stdout}` : "")
        : `${id}: ${result.stderr || "failed"}`;
    }, `${command.replaceAll("_", " ")}`);
    await refresh();
  }
</script>

<div class="dashboard">
  {#if toolState.error}
    <InlineBanner variant="info">{toolState.error}</InlineBanner>
  {/if}

  <div class="overview-grid dashboard-overview">
    <div class="stat-card">
      <div class="stat-icon-wrapper accent">
        <Globe size={18} />
      </div>
      <div class="stat-info">
        <span class="stat-label">Your local sites</span>
        <div class="stat-value-group">
          <span class="stat-value">{runningSitesCount} / {workspaces.length}</span>
          <span class="pill-badge" class:success={runningSitesCount > 0} class:warning={runningSitesCount === 0}>{runningSitesCount ? "Active" : "Ready to start"}</span>
        </div>
      </div>
    </div>
  </div>

  <!-- Site-first dashboard. Runtime state stays in the compact app footer. -->
  <section class="section">
    <div class="section-header">
      <div class="section-title-group">
        <Globe size={16} class="text-accent" />
        <h2 class="section-title">Start a site</h2>
      </div>
      <div class="dashboard-actions">
        <button class="btn-ghost btn-sm" onclick={refresh} disabled={toolState.busy} title="Refresh site status">
          <RefreshCw size={12} class={toolState.busy ? "spin" : ""} />
          <span>Refresh</span>
        </button>
        <button class="btn-link" onclick={onNavigateToSites}>
          <span>Manage sites</span>
          <ArrowRight size={13} />
        </button>
      </div>
    </div>

    {#if workspaces.length === 0 && workspacesLoaded}
      <div class="empty-card">
        <Globe size={32} class="text-subtle" />
        <p class="empty-title">You haven't added any sites yet</p>
        <p class="empty-desc">Go to the <strong>Sites</strong> tab to create and configure your first project.</p>
        <button class="btn-primary btn-sm mt-3" onclick={onNavigateToSites}>
          <span>Go to Sites</span>
          <ArrowRight size={13} />
        </button>
      </div>
    {:else}
      <div class="sites-summary-list">
        {#each workspaces.slice(0, 5) as ws (ws.id)}
          <div class="summary-site-item">
            <div class="status-dot-wrapper">
              <span class="status-dot" class:running={ws.running}></span>
            </div>
            <div class="site-main-info">
              <span class="site-name">{ws.name}</span>
              <span class="site-domain">{ws.domain}</span>
            </div>
            <div class="site-meta-badge">
              <span class="pill-badge {ws.preset?.toLowerCase() === 'wordpress' ? 'pink' : ws.preset?.toLowerCase() === 'node' ? 'sky' : 'purple'}">{ws.preset}</span>
            </div>
            <button class:running={ws.running} class="site-summary-toggle" onclick={() => toggleSite(ws)} disabled={toolState.busy}>
              {#if ws.running}<Square size={11} fill="currentColor" /><span>Stop</span>{:else}<Play size={11} fill="currentColor" /><span>Start</span>{/if}
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </section>
</div>

