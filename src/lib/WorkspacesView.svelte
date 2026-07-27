<script>
  // @ts-nocheck — Tauri IPC payloads are dynamically shaped JSON.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import BusyButton from "#lib/BusyButton.svelte";
  import InlineBanner from "#lib/InlineBanner.svelte";
  import WorkspaceCard from "#lib/WorkspaceCard.svelte";
  import { invokeWith } from "#lib/tauri-utils.svelte.js";
  import { Plus, Search, Filter, Globe, ChevronDown, ChevronUp, RefreshCw, SlidersHorizontal } from "@lucide/svelte";

  /** @type {any[]} */
  let presets = $state([]);
  /** @type {any[]} */
  let workspaces = $state([]);
  let workspacesLoaded = $state(false);
  let showCreateCard = $state(false);

  // Form state
  let name = $state("");
  let preset = $state("");
  let showSetup = $state(false);
  let wordpressVersion = $state("latest");
  let wordpressAdminUser = $state("admin");
  let wordpressAdminPassword = $state("");
  let wordpressAdminEmail = $state("");

  // Search & Filter state
  let searchQuery = $state("");
  let filterStatus = $state("all"); // 'all' | 'running' | 'stopped'

  /** @type {string[]} */
  let lastWarnings = $state([]);
  let error = $state("");
  let createState = $state({ busy: false, error: "", operation: "" });

  onMount(async () => {
    await loadData();
  });

  async function loadData() {
    try {
      const [pResults, wsResults] = await Promise.all([
        invoke("get_site_presets"),
        invoke("list_workspaces")
      ]);
      presets = pResults;
      workspaces = wsResults;
      workspacesLoaded = true;
      if (!preset && presets.length > 0) {
        preset = presets.find((/** @type {any} */ item) => item.value === "wordpress")?.value ?? presets[0]?.value ?? "";
      }
      // If no workspaces exist yet, auto-expand create card
      if (workspaces.length === 0) {
        showCreateCard = true;
      }
    } catch (e) {
      console.error(e);
    }
  }

  async function create() {
    if (!name.trim() || !preset || createState.busy) return;
    const isWordPress = preset.toLowerCase() === "wordpress";
    lastWarnings = [];
    await invokeWith(createState, async () => {
      const result = await invoke("create_workspace", {
        name: name.trim(),
        preset,
        options: {
          wordpressVersion,
          wordpressAdminUser: isWordPress ? wordpressAdminUser : "",
          wordpressAdminPassword: isWordPress ? wordpressAdminPassword : "",
          wordpressAdminEmail: isWordPress ? wordpressAdminEmail : "",
        },
      });
      lastWarnings = result.warnings ?? [];
      name = "";
      wordpressAdminPassword = "";
      showSetup = false;
      showCreateCard = false;
      await loadData();
    }, "Setting up site", { toastSuccess: "Site created successfully" });
    error = createState.error;
  }

  let filteredWorkspaces = $derived(
    workspaces.filter((ws) => {
      const matchesSearch =
        ws.name?.toLowerCase().includes(searchQuery.toLowerCase()) ||
        ws.domain?.toLowerCase().includes(searchQuery.toLowerCase()) ||
        ws.preset?.toLowerCase().includes(searchQuery.toLowerCase());
      if (filterStatus === "running") return matchesSearch && ws.running;
      if (filterStatus === "stopped") return matchesSearch && !ws.running;
      return matchesSearch;
    })
  );
</script>

<div class="workspaces-view">
  <!-- Top Action Bar -->
  <div class="action-bar">
    <div class="search-filter-group">
      <div class="search-box">
        <Search size={14} class="search-icon" />
        <input
          type="text"
          placeholder="Search site by name or domain..."
          bind:value={searchQuery}
        />
      </div>

      <div class="filter-pills">
        <button
          class="filter-pill"
          class:active={filterStatus === "all"}
          onclick={() => (filterStatus = "all")}
        >
          All ({workspaces.length})
        </button>
        <button
          class="filter-pill"
          class:active={filterStatus === "running"}
          onclick={() => (filterStatus = "running")}
        >
          Running ({workspaces.filter((w) => w.running).length})
        </button>
        <button
          class="filter-pill"
          class:active={filterStatus === "stopped"}
          onclick={() => (filterStatus = "stopped")}
        >
          Stopped ({workspaces.filter((w) => !w.running).length})
        </button>
      </div>
    </div>

    <button
      class="btn-create-toggle"
      class:active={showCreateCard}
      onclick={() => (showCreateCard = !showCreateCard)}
    >
      <Plus size={15} />
      <span>Create New Site</span>
      {#if showCreateCard}
        <ChevronUp size={14} />
      {:else}
        <ChevronDown size={14} />
      {/if}
    </button>
  </div>

  <!-- Create Site Accordion Drawer -->
  {#if showCreateCard}
    <div class="create-card-wrapper">
      <div class="create-card-header">
        <div class="create-title-group">
          <Globe size={18} class="text-accent" />
          <div>
            <h3 class="create-title">Add New Website</h3>
            <p class="create-desc">
              DevPanel will automatically configure the runtime environment, database, local domain and HTTPS certificate.
            </p>
          </div>
        </div>
      </div>

      <div class="create-form">
        <div class="form-row">
          <div class="field-group flex-2">
            <label for="site-name-input">Project name</label>
            <input
              id="site-name-input"
              placeholder="e.g. My Amazing Project"
              bind:value={name}
              disabled={createState.busy}
            />
          </div>

          <div class="field-group flex-1">
            <label for="site-preset-select">Technology preset</label>
            <select id="site-preset-select" bind:value={preset} disabled={createState.busy}>
              {#each presets as p (p.value)}
                <option value={p.value}>{p.label}</option>
              {/each}
            </select>
          </div>
        </div>

        <button
          class="btn-advanced-toggle"
          onclick={() => (showSetup = !showSetup)}
          disabled={createState.busy}
        >
          <SlidersHorizontal size={13} />
          <span>{showSetup ? "Hide advanced options" : "Configure advanced options"}</span>
        </button>

        {#if showSetup}
          <div class="setup-panel">
            {#if preset.toLowerCase() === "wordpress"}
              <div class="setup-title">WordPress Initial Setup</div>
              <div class="setup-grid">
                <label>
                  <span>WordPress version</span>
                  <select bind:value={wordpressVersion}>
                    <option value="latest">Latest stable</option>
                    <option value="6.7">6.7</option>
                    <option value="6.6">6.6</option>
                    <option value="6.5">6.5</option>
                  </select>
                </label>
                <label>
                  <span>Admin User</span>
                  <input bind:value={wordpressAdminUser} placeholder="admin" />
                </label>
                <label>
                  <span>Admin Password</span>
                  <input
                    type="password"
                    bind:value={wordpressAdminPassword}
                    autocomplete="new-password"
                    placeholder="Generate automatically"
                  />
                </label>
                <label>
                  <span>Admin Email</span>
                  <input type="email" bind:value={wordpressAdminEmail} placeholder="admin@domain.test" />
                </label>
              </div>
            {/if}
            <div class="setup-note">
              This site will use the global environment configured in Settings.
            </div>
          </div>
        {/if}

        <div class="form-actions">
          <button class="btn-ghost" onclick={() => (showCreateCard = false)} disabled={createState.busy}>
            Cancel
          </button>
          <BusyButton
            class="btn-create-submit"
            onclick={create}
            disabled={!name.trim() || !preset}
            busy={createState.busy}
            busyLabel="Creating site..."
          >
            <Plus size={14} />
            <span>Create Website</span>
          </BusyButton>
        </div>
      </div>
    </div>
  {/if}

  {#if error}
    <InlineBanner variant="danger">{error}</InlineBanner>
  {/if}

  {#if lastWarnings.length}
    <InlineBanner variant="warning">
      {#each lastWarnings as w}
        <div>{w}</div>
      {/each}
    </InlineBanner>
  {/if}

  <!-- Sites List Section -->
  <div class="sites-grid-container">
    {#if !workspacesLoaded}
      <div class="loading-state">
        <RefreshCw size={24} class="spin text-accent" />
        <p>Loading your sites...</p>
      </div>
    {:else if filteredWorkspaces.length === 0}
      <div class="empty-sites-state">
        <Globe size={40} class="text-subtle" />
        <h4>{searchQuery ? "No sites found for that search" : "You don't have any registered sites"}</h4>
        <p>{searchQuery ? "Try a different search term" : "Click 'Create New Site' above to get started."}</p>
      </div>
    {:else}
      <div class="sites-grid">
        {#each filteredWorkspaces as ws (ws.id)}
          <WorkspaceCard workspace={ws} onDeleted={loadData} onUpdated={loadData} />
        {/each}
      </div>
    {/if}
  </div>
</div>

