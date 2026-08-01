<script>
  // @ts-nocheck — Tauri IPC payloads are dynamically shaped JSON.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import BusyButton from "#lib/BusyButton.svelte";
  import InlineBanner from "#lib/InlineBanner.svelte";
  import WorkspaceCard from "#lib/WorkspaceCard.svelte";
  import { invokeWith } from "#lib/tauri-utils.svelte.js";
  import { Plus, Search, Globe, ChevronDown, ChevronUp, RefreshCw, SlidersHorizontal } from "@lucide/svelte";

  const setupModes = [
    { id: "custom", title: "Custom environment", summary: "Start with PHP, Node.js or Python." },
    { id: "starter", title: "Framework / starter", summary: "Create a framework project with a configurable runtime." },
    { id: "app", title: "CMS / web app", summary: "Install a ready-made web application." },
    { id: "existing", title: "Existing folder", summary: "Register an existing local project without moving it." },
  ];

  /** @type {any[]} */
  let presets = $state([]);
  /** @type {any[]} */
  let workspaces = $state([]);
  let unregisteredFolders = $state([]);
  let workspacesLoaded = $state(false);
  let showCreateCard = $state(false);

  // Form state
  let name = $state("");
  let preset = $state("");
  let projectMode = $state("custom");
  let simpleMode = $state(true);
  let customRuntime = $state("php");
  let starterPreset = $state("laravel");
  let appPreset = $state("wordpress");
  let existingRoot = $state("");
  let documentRoot = $state("");
  let createDatabase = $state(false);
  let advancedPreset = $state("");
  let runtimeCatalog = $state({ php_versions: [] });
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

  let selectedPreset = $derived(advancedPreset || (projectMode === "custom" ? customRuntime : projectMode === "starter" ? starterPreset : projectMode === "app" ? appPreset : "php"));

  onMount(async () => {
    await loadData();
  });

  async function loadData() {
    try {
      const [pResults, wsResults, runtimes, discovered] = await Promise.all([
        invoke("get_site_presets"),
        invoke("list_workspaces"),
        invoke("get_runtime_catalog").catch(() => ({ php_versions: [] })),
        invoke("discover_workspace_folders").catch(() => [])
      ]);
      presets = pResults;
      workspaces = wsResults;
      runtimeCatalog = runtimes;
      unregisteredFolders = discovered;
      workspacesLoaded = true;
      preset = selectedPreset;
      // If no workspaces exist yet, auto-expand create card
      if (workspaces.length === 0) {
        showCreateCard = true;
      }
    } catch (e) {
      console.error(e);
    }
  }

  function registerDetectedFolder(folder) {
    simpleMode = false;
    projectMode = "existing";
    name = folder.name;
    existingRoot = folder.path;
    documentRoot = folder.suggestedDocumentRoot || "";
    advancedPreset = folder.suggestedPreset || "php";
    showCreateCard = true;
    showSetup = true;
  }

  async function create() {
    const selected = selectedPreset;
    if (!name.trim() || !selected || createState.busy) return;
    const isWordPress = selected.toLowerCase() === "wordpress";
    lastWarnings = [];
    await invokeWith(createState, async () => {
      const result = await invoke("create_workspace", {
        name: name.trim(),
        preset: selected,
        options: {
          projectMode,
          externalRoot: projectMode === "existing" ? existingRoot : "",
          documentRoot,
          createDatabase: isWordPress || createDatabase,
          wordpressVersion,
          wordpressAdminUser: isWordPress ? wordpressAdminUser : "",
          wordpressAdminPassword: isWordPress ? wordpressAdminPassword : "",
          wordpressAdminEmail: isWordPress ? wordpressAdminEmail : "",
        },
      });
      lastWarnings = result.warnings ?? [];
      name = ""; existingRoot = ""; documentRoot = ""; createDatabase = false; advancedPreset = "";
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
      <span>Add New Website</span>
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
              Choose what you want to build. DevPanel creates its folder, local domain, HTTPS and database when the project needs one.
            </p>
          </div>
        </div>
      </div>

      <div class="create-form">
        <label class="toggle-switch setup-complexity-toggle">
          <input type="checkbox" bind:checked={simpleMode} />
          <span class="toggle-label">Simple setup</span>
          <small>{simpleMode ? "PHP, Laravel, WordPress and Node.js only" : "Show all project modes and user templates"}</small>
        </label>
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

        </div>

        <fieldset class="project-type-picker" disabled={createState.busy}>
          <legend>Choose your project setup</legend>
          <div class="project-type-grid">
            {#each (simpleMode ? setupModes.filter((mode) => mode.id !== "existing") : setupModes) as mode (mode.id)}
              <button
                type="button"
                class:active={projectMode === mode.id}
                class="project-type-card"
                onclick={() => { projectMode = mode.id; advancedPreset = ""; }}
                aria-pressed={projectMode === mode.id}
              >
                <strong>{mode.title}</strong>
                <span>{mode.summary}</span>
              </button>
            {/each}
          </div>
        </fieldset>

        <div class="setup-panel">
          {#if projectMode === "custom"}
            <div class="setup-title">Custom environment</div>
            <div class="setup-grid">
              <label><span>Runtime</span><select bind:value={customRuntime}><option value="php">PHP</option><option value="astro">Node.js</option>{#if !simpleMode}<option value="python">Python</option>{/if}</select></label>
              <label><span>Document root</span><input bind:value={documentRoot} placeholder="Root folder (or public)" /></label>
              <label class="toggle-switch"><input type="checkbox" bind:checked={createDatabase} /><span class="toggle-label">Create database</span></label>
              {#if customRuntime === "php" && runtimeCatalog.php_versions?.length}
                <label><span>Installed PHP version</span><select disabled><option>{runtimeCatalog.php_versions.map((item) => item.label).join(", ")}</option></select><small>Choose a per-site PHP version after creation in Site configuration.</small></label>
              {/if}
            </div>
          {:else if projectMode === "starter"}
            <div class="setup-title">Framework / starter</div>
            <div class="setup-grid">
              <label><span>Starter</span><select bind:value={starterPreset}><option value="laravel">Laravel</option>{#if !simpleMode}<option value="astro">Astro</option><option value="symfony">Symfony</option><option value="express">Express.js</option><option value="nextjs">Next.js</option><option value="react">React</option><option value="django">Django</option><option value="flask">Flask</option>{/if}</select></label>
              <label><span>Document root</span><input bind:value={documentRoot} placeholder="Automatic (public for Laravel)" /></label>
              <label class="toggle-switch"><input type="checkbox" bind:checked={createDatabase} /><span class="toggle-label">Create database</span></label>
            </div>
          {:else if projectMode === "app"}
            <div class="setup-title">CMS / web app</div>
            <div class="setup-grid"><label><span>Application</span><select bind:value={appPreset}><option value="wordpress">WordPress</option>{#if !simpleMode}<option value="blesta">Blesta</option><option value="whmcs">WHMCS</option><option value="drupal">Drupal</option><option value="prestashop">PrestaShop</option><option value="magento">Magento Open Source</option><option value="joomla">Joomla</option>{/if}</select></label></div>
          {:else}
            <div class="setup-title">Existing local folder</div>
            <div class="setup-grid">
              <label><span>Project folder</span><input bind:value={existingRoot} placeholder="C:\\Projects\\my-app" /></label>
              <label><span>Document root</span><input bind:value={documentRoot} placeholder="Root folder (or public)" /></label>
              <label class="toggle-switch"><input type="checkbox" bind:checked={createDatabase} /><span class="toggle-label">Create database</span></label>
            </div>
          {/if}
          {#if !simpleMode}
            <label class="runtime-field"><span>User template from site-presets.conf</span><select bind:value={advancedPreset}><option value="">Use the selected built-in option</option>{#each presets as item (item.value)}<option value={item.value}>{item.label}</option>{/each}</select></label>
          {/if}
          <div class="setup-note">Runtime source and versions are chosen in Modules. DevPanel does not modify imported folders.</div>
        </div>

        <button
          class="btn-advanced-toggle"
          onclick={() => (showSetup = !showSetup)}
          disabled={createState.busy}
        >
          <SlidersHorizontal size={13} />
          <span>{showSetup ? "Hide advanced options" : "Configure advanced options"}</span>
        </button>

        {#if showSetup && selectedPreset.toLowerCase() === "wordpress"}
          <div class="setup-panel">
            {#if selectedPreset.toLowerCase() === "wordpress"}
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
          </div>
        {/if}

        <div class="form-actions">
          <button class="btn-ghost" onclick={() => (showCreateCard = false)} disabled={createState.busy}>
            Cancel
          </button>
          <BusyButton
            class="btn-create-submit"
            onclick={create}
            disabled={!name.trim() || (projectMode === "existing" && !existingRoot.trim())}
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

  {#if unregisteredFolders.length}
    <InlineBanner variant="info">
      <strong>Folders ready to register</strong>
      <div>DevPanel found project folders in <code>www</code>. Registering keeps each folder in place and only adds its Site configuration.</div>
      <div class="detected-folder-actions">
        {#each unregisteredFolders as folder (folder.path)}
          <button class="btn-subtle" onclick={() => registerDetectedFolder(folder)}>
            Register {folder.name}
          </button>
        {/each}
      </div>
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

