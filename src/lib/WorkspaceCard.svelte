<script>
  import { invoke } from "@tauri-apps/api/core";
  import { openPath, openUrl } from "@tauri-apps/plugin-opener";
  import ConfirmDialog from "#lib/ConfirmDialog.svelte";
  import DebugDialog from "#lib/DebugDialog.svelte";
  import WordPressSitePanel from "#lib/WordPressSitePanel.svelte";
  import { invokeWith } from "#lib/tauri-utils.svelte.js";
  import Loader from "#lib/Loader.svelte";
  import BusyButton from "#lib/BusyButton.svelte";
  import DropdownMenu from "#lib/DropdownMenu.svelte";
  import DetailsOutput from "#lib/DetailsOutput.svelte";
  import { notify } from "#lib/notifications.svelte.js";
  import {
    Globe,
    ExternalLink,
    Folder,
    Terminal,
    Play,
    Square,
    Shield,
    ShieldCheck,
    EllipsisVertical,
    Database,
    Code,
    Wrench,
    Trash2,
    Check,
    TriangleAlert,
    KeyRound
  } from "@lucide/svelte";

  /** @typedef {{ site_path: string, php_ini_path: string | null, mysql_data_path: string, composer_config_path: string | null, redis_config_path: string | null, memcached_config_path: string | null, sendmail_path: string | null, heidisql_available: boolean, cmder_available: boolean, phpmyadmin_available: boolean }} SitePaths */

  let { workspace, onDeleted, onUpdated } = $props();

  let error = $state("");
  let showMenu = $state(false);
  let showDebug = $state(false);
  /** @type {"configuration" | "database" | "wordpress" | null} */
  let detailPanel = $state(null);
  let confirmDelete = $state(false);
  /** @type {SitePaths | null} */
  let paths = $state(null);
  /** @type {any | null} */
  let runtimeCatalog = $state(null);
  /** @type {any | null} */
  let runtimeProfile = $state(null);
  let phpOverrideUnavailable = $state(false);
  let toggleState = $state({ busy: false, error: "", operation: "" });
  let profileState = $state({ busy: false, error: "", operation: "" });

  function isWordPressSite() {
    return workspace.preset?.toLowerCase() === "wordpress";
  }

  async function toggleSite() {
    error = "";
    await invokeWith(toggleState, async () => {
      await invoke(workspace.running ? "stop_workspace" : "start_workspace", { id: workspace.id });
      onUpdated?.();
    }, workspace.running ? "Stopping" : "Starting", { toastSuccess: workspace.running ? "Site stopped" : "Site started" });
  }

  /** @returns {Promise<SitePaths>} */
  async function sitePaths() {
    if (!paths) paths = await invoke("get_workspace_paths", { id: workspace.id });
    return /** @type {SitePaths} */ (paths);
  }

  /** @param {"site" | "admin" | "folder" | "php" | "mysql" | "composer" | "redis" | "memcached" | "sendmail" | "heidisql" | "cmder" | "vscode" | "cursor" | "sublime" | "claude" | "codex"} action */
  async function runTool(action) {
    error = "";
    try {
      if (action === "site") {
        await openUrl(`${workspace.https_ready ? "https" : "http"}://${workspace.domain}`);
      } else if (action === "admin") {
        await openUrl(`${workspace.https_ready ? "https" : "http"}://${workspace.domain}/wp-admin/`);
      } else if (action === "folder") {
        await invoke("open_workspace_folder", { id: workspace.id });
      } else if (action === "composer") {
        const path = (await sitePaths()).composer_config_path;
        if (!path) throw new Error("Could not find composer.json for this site.");
        await openPath(path);
      } else if (action === "heidisql" || action === "cmder") {
        await invoke("launch_workspace_tool", { id: workspace.id, tool: action });
      } else if (["vscode", "cursor", "sublime", "claude", "codex"].includes(action)) {
        await invoke("launch_workspace_editor", { id: workspace.id, editor: action });
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function repairSite() {
    error = "";
    await invokeWith(toggleState, async () => {
      await invoke("retry_database_setup", { id: workspace.id });
      onUpdated?.();
    }, "Repairing");
  }

  async function configureHttps() {
    error = "";
    await invokeWith(toggleState, async () => {
      const warnings = await invoke("finish_domain_setup", { id: workspace.id });
      if (warnings.length) error = warnings.join("\n");
      onUpdated?.();
    }, "Configurando HTTPS");
  }

  /** @param {{ keepDatabase?: boolean } | undefined} opts */
  async function deleteSite(opts) {
    confirmDelete = false;
    error = "";
    try {
      if (opts?.keepDatabase) {
        await invoke("uninstall_workspace_keep_data", { id: workspace.id });
        notify("Site deleted (database preserved)", "success");
      } else {
        await invoke("delete_workspace_all", { id: workspace.id });
        notify("Site deleted permanently", "success");
      }
      onDeleted?.();
    } catch (e) {
      error = String(e);
    }
  }

  async function loadRuntimeProfile() {
    await invoke("refresh_runtime_detection");
    const [catalog, stacks, activeStackId] = await Promise.all([
      invoke("get_runtime_catalog"),
      invoke("get_stacks"),
      invoke("get_active_stack"),
    ]);
    runtimeCatalog = catalog;
    const activeStack = /** @type {any[]} */ (stacks).find((/** @type {any} */ stack) => stack.id === activeStackId);
    phpOverrideUnavailable = activeStack?.web_role?.Direct === "apache"
      || activeStack?.web_role?.direct === "apache";
    runtimeProfile = {
      php_version: workspace.runtime_profile?.php_version ?? "inherit",
    };
  }

  async function saveRuntimeProfile() {
    if (!runtimeProfile) return;
    error = "";
    await invokeWith(profileState, async () => {
      await invoke("set_workspace_runtime_profile", { id: workspace.id, profile: runtimeProfile });
      onUpdated?.();
    }, "Guardando");
  }

  /** @param {"configuration" | "database" | "wordpress"} panel */
  async function showDetails(panel) {
    detailPanel = detailPanel === panel ? null : panel;
    showMenu = false;
    if (detailPanel === "database") await sitePaths();
    if (detailPanel === "configuration") await loadRuntimeProfile();
  }
</script>

<article class="site-card" class:running={workspace.running}>
  <header class="card-header">
    <div class="site-title-box">
      <div class="status-indicator">
        <span class="dot" class:running={workspace.running}></span>
      </div>
      <div class="site-headings">
        <h3 class="site-name">{workspace.name}</h3>
        <span class="site-domain">{workspace.domain}</span>
      </div>
    </div>

    <div class="header-right">
      <span class="preset-tag">{workspace.preset}</span>
      <DropdownMenu bind:open={showMenu} label="Site actions" title="Site actions">
        <div class="menu-group-label">Detalles</div>
        <button onclick={() => showDetails("configuration")}>Site Configuration</button>
        <button onclick={() => showDetails("database")}>Database</button>
        {#if isWordPressSite()}<button onclick={() => showDetails("wordpress")}>WP Tools</button>{/if}
        <div class="menu-sep"></div>
        <div class="menu-group-label">Open in editor</div>
        <div class="menu-editors">
          <button onclick={() => runTool("vscode")}>VS Code</button>
          <button onclick={() => runTool("cursor")}>Cursor</button>
          <button onclick={() => runTool("sublime")}>Sublime</button>
          <button onclick={() => runTool("claude")}>Claude</button>
          <button onclick={() => runTool("codex")}>Codex</button>
        </div>
        <div class="menu-sep"></div>
        <div class="menu-group-label">Maintenance</div>
        <button onclick={repairSite} disabled={toggleState.busy}>Repair database</button>
        <button onclick={() => { showMenu = false; showDebug = true; }}>Diagnostics & logs</button>
        <div class="menu-sep"></div>
        <button class="danger" onclick={() => { showMenu = false; confirmDelete = true; }}>Delete site</button>
      </DropdownMenu>
    </div>
  </header>

  <div class="site-badges">
    <div class="badge" class:secure={workspace.https_ready}>
      {#if workspace.https_ready}
        <ShieldCheck size={12} />
        <span>HTTPS</span>
      {:else}
        <Shield size={12} />
        <span>HTTP</span>
      {/if}
    </div>

    <div class="badge status-badge" class:running={workspace.running}>
      <span>{workspace.running ? "Running" : "Stopped"}</span>
    </div>
  </div>

  <BusyButton
    class={`site-toggle-btn ${workspace.running ? "stop" : "start"}`}
    onclick={toggleSite}
    busy={toggleState.busy}
    busyLabel={toggleState.operation || "Processing..."}
  >
    {#if workspace.running}
      <Square size={13} fill="currentColor" />
      <span>Stop Site</span>
    {:else}
      <Play size={13} fill="currentColor" />
      <span>Start Site</span>
    {/if}
  </BusyButton>

  <div class="quick-actions-bar">
    <button class="action-btn" onclick={() => runTool("site")} disabled={!workspace.setup_complete} title="Open in browser">
      <ExternalLink size={12} />
      <span>Site</span>
    </button>
    {#if isWordPressSite()}
      <button class="action-btn" onclick={() => runTool("admin")} disabled={!workspace.setup_complete} title="Open WP Admin">
        <KeyRound size={12} />
        <span>Admin</span>
      </button>
    {/if}
    <button class="action-btn" onclick={() => runTool("folder")} title="Open folder">
      <Folder size={12} />
      <span>Folder</span>
    </button>
    <button class="action-btn" onclick={() => runTool("cmder")} title="Open terminal">
      <Terminal size={12} />
      <span>Shell</span>
    </button>
  </div>

  {#if detailPanel === "configuration"}
    <div class="details-panel">
      <div class="details-header">
        <Code size={14} class="text-accent" />
        <h4 class="details-title">General Configuration</h4>
      </div>
      <div class="detail-section">
        <div class="detail-row"><span>Domain</span><code>{workspace.domain}</code></div>
        <div class="detail-row"><span>Protocol</span><strong class={workspace.https_ready ? "txt-green" : ""}>{workspace.https_ready ? "HTTPS" : "HTTP"}</strong></div>
        <div class="detail-row"><span>Preset</span><strong>{workspace.preset}</strong></div>
      </div>
      {#if runtimeProfile && runtimeCatalog}
        <div class="detail-section">
          <div class="detail-section-title">Runtime de PHP</div>
          <label class="runtime-field">
            <span>PHP Version</span>
            <select bind:value={runtimeProfile.php_version} disabled={workspace.running || phpOverrideUnavailable}>
              <option value="inherit">Use default version</option>
              {#each runtimeCatalog.php_versions as item (item.value)}
                <option value={item.value}>{item.label}</option>
              {/each}
            </select>
          </label>
          {#if phpOverrideUnavailable}
            <div class="tool-note">Per-site PHP versions require the Nginx stack.</div>
          {:else if workspace.running}
            <div class="tool-note">Stop this site before changing its PHP version.</div>
          {:else}
            <button class="btn-subtle" onclick={saveRuntimeProfile} disabled={profileState.busy}>
              {profileState.busy ? "Saving..." : "Save PHP Version"}
            </button>
          {/if}
        </div>
      {/if}
      {#if workspace.wordpress_admin}
        <div class="detail-section">
          <div class="detail-section-title">Administrador WordPress</div>
          <div class="detail-row"><span>Username</span><code>{workspace.wordpress_admin.username}</code></div>
          <div class="detail-row"><span>Password</span><code>{workspace.wordpress_admin.password}</code></div>
          <div class="detail-row"><span>Email</span><code>{workspace.wordpress_admin.email}</code></div>
        </div>
      {/if}
      <div class="detail-section">
        <button class="btn-subtle" onclick={configureHttps} disabled={toggleState.busy}>Configure HTTPS</button>
      </div>
    </div>
  {:else if detailPanel === "database"}
    <div class="details-panel">
      <div class="details-header">
        <Database size={14} class="text-accent" />
        <h4 class="details-title">Database</h4>
      </div>
      <div class="detail-section">
        <div class="detail-row"><span>DB Name</span><code>{workspace.db_name}</code></div>
      </div>
      <div class="detail-section">
        <div class="detail-section-title">DB Tools</div>
        <button class="btn-subtle" onclick={() => runTool("heidisql")}>Open HeidiSQL</button>
        <button class="btn-subtle" disabled={!paths?.phpmyadmin_available}>Open phpMyAdmin</button>
        {#if !paths?.phpmyadmin_available}<div class="tool-note">phpMyAdmin is not installed in DevPanel/bin yet.</div>{/if}
        <button class="btn-subtle" onclick={() => runTool("composer")}>Open composer.json</button>
      </div>
    </div>
  {:else if detailPanel === "wordpress"}
    <WordPressSitePanel {workspace} onUpdated={onUpdated} />
  {/if}

  {#if toggleState.error || profileState.error || error}
    <DetailsOutput title="Site Diagnostics" value={toggleState.error || profileState.error || error} />
  {/if}
</article>

{#if confirmDelete}
  <ConfirmDialog
    message={`Permanently delete "${workspace.name}" and all its data?`}
    confirmLabel="Delete site"
    requireTextInput={workspace.name}
    showKeepDatabase={true}
    onConfirm={deleteSite}
    onCancel={() => (confirmDelete = false)}
  />
{/if}

{#if showDebug}
  <DebugDialog {workspace} onClose={() => (showDebug = false)} />
{/if}

