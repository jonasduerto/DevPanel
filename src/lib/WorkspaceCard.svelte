<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { openPath, openUrl } from "@tauri-apps/plugin-opener";
  import { onMount, onDestroy } from "svelte";
  import ConfirmDialog from "#lib/ConfirmDialog.svelte";
  import DebugDialog from "#lib/DebugDialog.svelte";
  import Dialog from "#lib/Dialog.svelte";
  import WordPressSitePanel from "#lib/WordPressSitePanel.svelte";
  import { invokeWith } from "#lib/tauri-utils.svelte.js";
  import Loader from "#lib/Loader.svelte";
  import BusyButton from "#lib/BusyButton.svelte";
  import DropdownMenu from "#lib/DropdownMenu.svelte";
  import DetailsOutput from "#lib/DetailsOutput.svelte";
  import { notify } from "#lib/notifications.svelte.js";
  import {
    ExternalLink,
    Folder,
    Terminal,
    Play,
    Square,
    Shield,
    ShieldCheck,
    Database,
    Code,
    Wrench,
    Check,
    TriangleAlert,
    KeyRound
  } from "@lucide/svelte";

  /** @typedef {{ site_path: string, php_ini_path: string | null, mysql_data_path: string, composer_config_path: string | null, redis_config_path: string | null, memcached_config_path: string | null, sendmail_path: string | null, heidisql_available: boolean, cmder_available: boolean, phpmyadmin_available: boolean }} SitePaths */

  let { workspace, onDeleted, onUpdated } = $props();

  const editorLabels = /** @type {Record<string, string>} */ ({ vscode: "VS Code", cursor: "Cursor", sublime: "Sublime", claude: "Claude", codex: "Codex" });

  let error = $state("");
  let showMenu = $state(false);
  let editorMenuOpen = $state(false);
  /** @type {string[]} */
  let availableEditors = $state([]);
  let showDebug = $state(false);
  /** @type {"configuration" | "database" | "tools" | "wordpress" | null} */
  let detailPanel = $state(null);
  /** @type {"dev" | "database" | "packages"} */
  let toolsTab = $state("dev");
  let confirmDelete = $state(false);
  /** @type {SitePaths | null} */
  let paths = $state(null);
  /** @type {any | null} */
  let runtimeCatalog = $state(null);
  /** @type {any | null} */
  let runtimeProfile = $state(null);
  /** @type {any | null} */
  let siteSettings = $state(null);
  /** @type {any | null} */
  let laravelEnvironment = $state(null);
  /** @type {any | null} */
  let projectCapabilities = $state(null);
  let projectTaskOutput = $state("");
  /** @type {Record<string, string>} key = "source:script" -> live output */
  let scriptOutputs = $state({});
  /** @type {Set<string>} keys of scripts currently running ("source:script") */
  let runningScripts = $state(new Set());
  /** @type {(() => void)[]} */
  let scriptEventUnlisteners = [];
  let phpOverrideUnavailable = $state(false);
  let toggleState = $state({ busy: false, error: "", operation: "" });
  let profileState = $state({ busy: false, error: "", operation: "" });
  let settingsState = $state({ busy: false, error: "", operation: "" });
  let laravelState = $state({ busy: false, error: "", operation: "" });
  let projectTaskState = $state({ busy: false, error: "", operation: "" });
  let showLaravelPassword = $state(false);
  let preferredEditor = $state("vscode");

  onMount(async () => {
    try {
      const config = await invoke("get_config");
      preferredEditor = config.preferred_editor || "vscode";
      availableEditors = await invoke("get_available_editors");
    } catch (e) {
      error = String(e);
    }
  });

  function isWordPressSite() {
    return workspace.preset?.toLowerCase() === "wordpress";
  }

  function isLaravelSite() {
    return workspace.preset?.toLowerCase() === "laravel";
  }

  function usesDatabase() {
    return workspace.requires_database || ["wordpress", "laravel", "blesta", "whmcs"].includes(workspace.preset?.toLowerCase());
  }

  function phpBadgeLabel() {
    const version = workspace.runtime_profile?.php_version;
    return version && version !== "inherit" ? `PHP ${version}` : "PHP default";
  }

  /** @param {string} id */
  function editorLabel(id) {
    return editorLabels[id] || id;
  }

  function fileManagerLabel() {
    const ua = navigator.userAgent.toLowerCase();
    if (ua.includes("macintosh") || ua.includes("darwin") || ua.includes("mac os")) return "Finder";
    if (ua.includes("linux")) return "Files";
    return "Explorer";
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

  /** @param {string} action */
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

  async function configureHttps() {
    error = "";
    await invokeWith(toggleState, async () => {
      const warnings = await invoke("finish_domain_setup", { id: workspace.id });
      if (warnings.length) error = warnings.join("\n");
      onUpdated?.();
      detailPanel = null;
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
    siteSettings = {
      domain: workspace.domain,
      documentRoot: workspace.document_root ?? "",
      dbName: workspace.db_name,
    };
    projectCapabilities = await invoke("get_project_capabilities", { id: workspace.id });
    if (isLaravelSite()) {
      laravelEnvironment = await invoke("get_laravel_environment", { id: workspace.id });
    }
  }

  async function saveRuntimeProfile() {
    if (!runtimeProfile) return;
    error = "";
    await invokeWith(profileState, async () => {
      await invoke("set_workspace_runtime_profile", { id: workspace.id, profile: runtimeProfile });
      onUpdated?.();
      detailPanel = null;
    }, "Guardando");
  }

  async function saveSiteSettings() {
    if (!siteSettings) return;
    error = "";
    await invokeWith(settingsState, async () => {
      const result = await invoke("update_workspace_settings", {
        id: workspace.id,
        settings: siteSettings,
      });
      if (result.warnings?.length) error = result.warnings.join("\n");
      await onUpdated?.();
      detailPanel = null;
    }, "Saving site settings", { toastSuccess: "Site settings saved" });
  }

  async function saveLaravelEnvironment() {
    if (!laravelEnvironment) return;
    error = "";
    await invokeWith(laravelState, async () => {
      await invoke("save_laravel_environment", {
        id: workspace.id,
        environment: laravelEnvironment,
      });
      detailPanel = null;
    }, "Saving Laravel environment", { toastSuccess: ".env saved" });
  }

  async function provisionSiteDatabase() {
    error = "";
    await invokeWith(settingsState, async () => {
      await invoke("provision_workspace_database", { id: workspace.id });
      if (isLaravelSite()) {
        laravelEnvironment = await invoke("get_laravel_environment", { id: workspace.id });
      }
      await onUpdated?.();
      detailPanel = null;
    }, "Synchronizing database", { toastSuccess: "Database and project configuration synchronized" });
  }

  /** @param {string} task */
  async function runProjectTask(task) {
    projectTaskOutput = "";
    error = "";
    await invokeWith(projectTaskState, async () => {
      projectTaskOutput = await invoke("run_project_task", { id: workspace.id, task });
    }, "Running project task", { toastSuccess: "Project task completed" });
  }

  // Composer's own auto-run hooks (post-autoload-dump, post-update-cmd…) are
  // noise for a developer looking for "how do I run this project" — they're
  // never meant to be clicked by hand, so keep them out of the primary lists.
  const COMPOSER_HOOK_PREFIXES = ["pre-", "post-"];
  /** @param {{ composerScripts: { name: string, command: string }[] } | null} caps */
  function isComposerHook(name) {
    return COMPOSER_HOOK_PREFIXES.some((prefix) => name.startsWith(prefix));
  }
  /** @param {any} caps */
  function devScripts(caps) {
    return (caps?.composerScripts || []).filter((s) => !isComposerHook(s.name) && /dev|watch|serve/i.test(s.name));
  }
  /** @param {any} caps */
  function noiseScripts(caps) {
    return (caps?.composerScripts || []).filter((s) => isComposerHook(s.name));
  }
  /** @param {any} caps */
  function secondaryScripts(caps) {
    const dev = new Set(devScripts(caps).map((s) => s.name));
    return (caps?.composerScripts || []).filter((s) => !isComposerHook(s.name) && !dev.has(s.name));
  }

  /** @param {"js" | "composer" | "artisan"} source @param {string} script */
  async function startProjectScript(source, script) {
    error = "";
    const key = `${source}:${script}`;
    scriptOutputs = { ...scriptOutputs, [key]: "" };
    try {
      const outputEvent = `project-script-output::${workspace.id}::${source}::${script}`;
      const exitEvent = `project-script-exit::${workspace.id}::${source}::${script}`;
      const unlistenOutput = await listen(outputEvent, (event) => {
        scriptOutputs = { ...scriptOutputs, [key]: `${scriptOutputs[key] || ""}${event.payload}\n` };
      });
      const unlistenExit = await listen(exitEvent, () => {
        const next = new Set(runningScripts);
        next.delete(key);
        runningScripts = next;
        unlistenOutput();
        unlistenExit();
      });
      scriptEventUnlisteners.push(unlistenOutput, unlistenExit);
      await invoke("start_project_script", { id: workspace.id, source, script });
      runningScripts = new Set(runningScripts).add(key);
    } catch (e) {
      error = String(e);
    }
  }

  /** @param {"js" | "composer"} source @param {string} script */
  async function stopProjectScript(source, script) {
    error = "";
    try {
      await invoke("stop_project_script", { id: workspace.id, source, script });
    } catch (e) {
      error = String(e);
    }
  }

  onDestroy(() => {
    scriptEventUnlisteners.forEach((unlisten) => unlisten());
  });

  /** @param {"configuration" | "database" | "tools" | "wordpress"} panel */
  async function showDetails(panel) {
    detailPanel = detailPanel === panel ? null : panel;
    showMenu = false;
    if (detailPanel === "database") await sitePaths();
    if (detailPanel === "configuration") await loadRuntimeProfile();
    if (detailPanel === "tools") {
      toolsTab = "dev";
      projectCapabilities = await invoke("get_project_capabilities", { id: workspace.id });
    }
  }
</script>

<article class="site-card" class:running={workspace.running}>
  {#if workspace.path_missing}
    <div class="site-missing-warning">
      <TriangleAlert size={14} />
      <span>Project folder is missing. DevPanel kept this Site safely; restore the folder or remove its Site entry.</span>
    </div>
  {/if}
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
      <span class="preset-tag php-tag" title="Site PHP runtime">{phpBadgeLabel()}</span>
      <DropdownMenu bind:open={showMenu} label="Site actions" title="Site actions">
        <div class="menu-group-label">Site</div>
        <button onclick={() => showDetails("configuration")}>Site configuration</button>
        {#if isWordPressSite()}<button onclick={() => showDetails("wordpress")}>WP Tools</button>{/if}
        <div class="menu-sep"></div>
        <div class="menu-group-label">Diagnostics</div>
        <button onclick={() => { showMenu = false; showDebug = true; }}>Diagnostics & logs</button>
        <div class="menu-sep"></div>
        <button class="danger" onclick={() => { showMenu = false; confirmDelete = true; }}>Delete site</button>
      </DropdownMenu>
    </div>
  </header>

  <div class="site-badges">
    <button class="badge ssl-badge" class:secure={workspace.https_ready} onclick={() => showDetails("configuration")} title="Open certificate and HTTPS settings">
      {#if workspace.https_ready}
        <ShieldCheck size={12} />
        <span>HTTPS</span>
      {:else}
        <Shield size={12} />
        <span>HTTP</span>
      {/if}
    </button>

    <div class="badge status-badge" class:running={workspace.running}>
      <span>{workspace.running ? "Running" : "Stopped"}</span>
    </div>
  </div>

  <BusyButton
    class={`site-toggle-btn ${workspace.running ? "stop" : "start"}`}
    onclick={toggleSite}
    busy={toggleState.busy}
    busyLabel={toggleState.operation || "Processing..."}
    title={workspace.running ? "Stop this site and release unused dependencies" : "Start this site and its required enabled dependencies"}
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
    <button class="action-btn" onclick={() => runTool("site")} disabled={!workspace.setup_complete || workspace.path_missing} title="Open site in browser">
      <ExternalLink size={14} />
    </button>
    {#if isWordPressSite()}
      <button class="action-btn" onclick={() => runTool("admin")} disabled={!workspace.setup_complete || workspace.path_missing} title="Open WordPress Admin">
        <KeyRound size={14} />
      </button>
    {/if}
    <DropdownMenu bind:open={editorMenuOpen} label="Open project in editor or file manager" title="Open project in editor or file manager" align="left" triggerClass="action-btn">
      {#snippet trigger()}<Code size={14} />{/snippet}
      {#if availableEditors.length}
        <div class="menu-group-label">Open with</div>
        <div class="menu-editors">
          {#each availableEditors as editor (editor)}
            <button class:active={editor === preferredEditor} onclick={() => { editorMenuOpen = false; runTool(editor); }}>
              {#if editor === preferredEditor}<Check size={11} />{/if}{editorLabel(editor)}
            </button>
          {/each}
        </div>
        <div class="menu-sep"></div>
      {/if}
      <div class="menu-group-label">Open project folder</div>
      <div class="menu-open-folder">
        <button onclick={() => { editorMenuOpen = false; runTool("folder"); }}>
          <Folder size={12} />
          <span>{fileManagerLabel()}</span>
        </button>
      </div>
    </DropdownMenu>
    <span class="action-sep"></span>
    {#if usesDatabase()}
      <button class="action-btn" onclick={() => showDetails("database")} title="Database tools">
        <Database size={14} />
      </button>
    {/if}
    <button class="action-btn" onclick={() => showDetails("tools")} title="Project tools">
      <Wrench size={14} />
    </button>
    <button class="action-btn" onclick={() => runTool("cmder")} title="Open terminal (Cmder)">
      <Terminal size={14} />
    </button>
  </div>

  {#if detailPanel === "configuration"}
    <Dialog
      title="General Configuration"
      width="min(560px, calc(100vw - 32px))"
      maxHeight="90%"
      scrollable
      onClose={() => (detailPanel = null)}
    >
    <div class="details-panel">
      <div class="detail-section">
        <div class="detail-row"><span>Domain</span><code>{workspace.domain}</code></div>
        <div class="detail-row"><span>Protocol</span><strong class={workspace.https_ready ? "txt-green" : ""}>{workspace.https_ready ? "HTTPS" : "HTTP"}</strong></div>
        <div class="detail-row"><span>Preset</span><strong>{workspace.preset}</strong></div>
      </div>
      {#if siteSettings}
        <div class="detail-section">
          <div class="detail-section-title">Site binding</div>
          <label class="runtime-field"><span>Local domain</span><input bind:value={siteSettings.domain} disabled={workspace.running} placeholder="my-project.dev" /></label>
          <label class="runtime-field"><span>Document root</span><input bind:value={siteSettings.documentRoot} disabled={workspace.running} placeholder="public" /></label>
          <label class="runtime-field"><span>Database name</span><input bind:value={siteSettings.dbName} disabled={workspace.running} placeholder="project_db" /></label>
          {#if workspace.running}
            <div class="tool-note">Stop this site before changing its domain, document root or database binding.</div>
          {:else}
            <button class="btn-subtle" onclick={saveSiteSettings} disabled={settingsState.busy}>
              {settingsState.busy ? "Saving..." : "Save Site Binding"}
            </button>
            <button class="btn-subtle" onclick={provisionSiteDatabase} disabled={settingsState.busy}>Initialize / Sync DevPanel Database</button>
            <div class="tool-note">Sync creates the DevPanel database/user and aligns Laravel .env. Changing a domain requires Configure HTTPS again.</div>
          {/if}
        </div>
      {/if}
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
      {#if isLaravelSite() && laravelEnvironment}
        <div class="detail-section">
          <div class="detail-section-title">Laravel .env</div>
          <label class="runtime-field"><span>APP_URL</span><input bind:value={laravelEnvironment.app_url} disabled={workspace.running} /></label>
          <div class="laravel-env-grid">
            <label class="runtime-field"><span>DB connection</span><input bind:value={laravelEnvironment.db_connection} disabled={workspace.running} /></label>
            <label class="runtime-field"><span>DB host</span><input bind:value={laravelEnvironment.db_host} disabled={workspace.running} /></label>
            <label class="runtime-field"><span>DB port</span><input bind:value={laravelEnvironment.db_port} disabled={workspace.running} /></label>
            <label class="runtime-field"><span>DB database</span><input bind:value={laravelEnvironment.db_database} disabled={workspace.running} /></label>
            <label class="runtime-field"><span>DB username</span><input bind:value={laravelEnvironment.db_username} disabled={workspace.running} /></label>
            <label class="runtime-field"><span>DB password</span><input type={showLaravelPassword ? "text" : "password"} bind:value={laravelEnvironment.db_password} disabled={workspace.running} /></label>
          </div>
          {#if workspace.running}
            <div class="tool-note">Stop this site before editing .env values.</div>
          {:else}
            <button class="btn-subtle" onclick={saveLaravelEnvironment} disabled={laravelState.busy}>
              {laravelState.busy ? "Saving..." : "Save Laravel .env"}
            </button>
            <button class="btn-subtle" onclick={() => (showLaravelPassword = !showLaravelPassword)}>
              {showLaravelPassword ? "Hide password" : "Show password"}
            </button>
          {/if}
        </div>
      {/if}
      <div class="detail-section">
        <button class="btn-subtle" onclick={configureHttps} disabled={toggleState.busy}>Configure HTTPS</button>
      </div>
    </div>
    </Dialog>
  {:else if detailPanel === "database"}
    <Dialog
      title="Database"
      width="min(440px, calc(100vw - 32px))"
      onClose={() => (detailPanel = null)}
    >
    <div class="details-panel">
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
    </Dialog>
  {:else if detailPanel === "tools"}
    <Dialog
      title="Project Tools"
      width="min(560px, calc(100vw - 32px))"
      maxHeight="90%"
      scrollable
      onClose={() => (detailPanel = null)}
    >
    {#if projectCapabilities}
      <div class="tab-nav">
        <button class:active={toolsTab === "dev"} onclick={() => (toolsTab = "dev")}>⚡ Scripts & Dev</button>
        <button class:active={toolsTab === "database"} onclick={() => (toolsTab = "database")}>🗄️ Database & Cache</button>
        <button class:active={toolsTab === "packages"} onclick={() => (toolsTab = "packages")}>📦 Packages & Environment</button>
      </div>

      {#if toolsTab === "dev"}
        <div class="tab-content details-panel">
          {#if projectCapabilities.packageJson}
            <div class="detail-section">
              <div class="detail-section-title">JS scripts ({projectCapabilities.jsRunner})</div>
              {#if projectCapabilities.jsScripts.length}
                <div class="project-task-row">
                  {#each projectCapabilities.jsScripts as scriptEntry (scriptEntry.name)}
                    {@const key = `js:${scriptEntry.name}`}
                    {#if runningScripts.has(key)}
                      <button class="btn-subtle danger" onclick={() => stopProjectScript("js", scriptEntry.name)} title={scriptEntry.command}>■ Stop {scriptEntry.name}</button>
                    {:else}
                      <button class="btn-subtle" onclick={() => startProjectScript("js", scriptEntry.name)} disabled={!projectCapabilities.jsRunnerAvailable} title={`${projectCapabilities.jsRunner} run ${scriptEntry.name} — ${scriptEntry.command}`}>{projectCapabilities.jsRunner} run {scriptEntry.name}</button>
                    {/if}
                  {/each}
                </div>
                {#each projectCapabilities.jsScripts as scriptEntry (scriptEntry.name)}
                  {@const key = `js:${scriptEntry.name}`}
                  {#if scriptOutputs[key]}<DetailsOutput title={`${scriptEntry.name} output`} value={scriptOutputs[key]} />{/if}
                {/each}
              {:else}
                <div class="tool-note">Sin scripts JS detectados</div>
              {/if}
              {#if !projectCapabilities.jsRunnerAvailable}<div class="tool-note" title="Falta el ejecutable {projectCapabilities.jsRunner}">Falta el ejecutable {projectCapabilities.jsRunner}. Install it in DevPanel/bin first.</div>{/if}
            </div>
          {/if}

          {#if projectCapabilities.artisan}
            <div class="detail-section">
              <div class="detail-section-title">Laravel watchers</div>
              <div class="project-task-row">
                {#each [{ key: "serve", label: "php artisan serve" }, { key: "queue:work", label: "php artisan queue:work" }] as watcher (watcher.key)}
                  {@const rkey = `artisan:${watcher.key}`}
                  {#if runningScripts.has(rkey)}
                    <button class="btn-subtle danger" onclick={() => stopProjectScript("artisan", watcher.key)}>■ Stop {watcher.key}</button>
                  {:else}
                    <button class="btn-subtle" onclick={() => startProjectScript("artisan", watcher.key)} disabled={!projectCapabilities.devpanelPhpAvailable}>{watcher.label}</button>
                  {/if}
                {/each}
              </div>
              {#each [{ key: "serve" }, { key: "queue:work" }] as watcher (watcher.key)}
                {@const rkey = `artisan:${watcher.key}`}
                {#if scriptOutputs[rkey]}<DetailsOutput title={`${watcher.key} output`} value={scriptOutputs[rkey]} />{/if}
              {/each}
            </div>
          {/if}

          {#if devScripts(projectCapabilities).length}
            <div class="detail-section">
              <div class="detail-section-title">Composer dev scripts</div>
              <div class="project-task-row">
                {#each devScripts(projectCapabilities) as scriptEntry (scriptEntry.name)}
                  {@const key = `composer:${scriptEntry.name}`}
                  {#if runningScripts.has(key)}
                    <button class="btn-subtle danger" onclick={() => stopProjectScript("composer", scriptEntry.name)} title={scriptEntry.command}>■ Stop {scriptEntry.name}</button>
                  {:else}
                    <button class="btn-subtle" onclick={() => startProjectScript("composer", scriptEntry.name)} disabled={!projectCapabilities.devpanelComposerAvailable} title={scriptEntry.command}>composer run {scriptEntry.name}</button>
                  {/if}
                {/each}
              </div>
              {#each devScripts(projectCapabilities) as scriptEntry (scriptEntry.name)}
                {@const key = `composer:${scriptEntry.name}`}
                {#if scriptOutputs[key]}<DetailsOutput title={`${scriptEntry.name} output`} value={scriptOutputs[key]} />{/if}
              {/each}
            </div>
          {/if}

          {#if !projectCapabilities.packageJson && !projectCapabilities.artisan && !devScripts(projectCapabilities).length}
            <div class="tool-note">No dev scripts detected for this project.</div>
          {/if}
        </div>
      {:else if toolsTab === "database"}
        <div class="tab-content details-panel">
          {#if projectCapabilities.artisan}
            <div class="detail-section">
              <div class="detail-section-title">Migrations</div>
              <div class="project-task-row">
                <button class="btn-subtle" onclick={() => runProjectTask("artisan_migrate")} disabled={projectTaskState.busy}>Migrate</button>
              </div>
            </div>
            <div class="detail-section">
              <div class="detail-section-title">Cache</div>
              <div class="project-task-row">
                <button class="btn-subtle" onclick={() => runProjectTask("artisan_cache_clear")} disabled={projectTaskState.busy}>Clear all cache</button>
                <button class="btn-subtle" onclick={() => runProjectTask("artisan_config_clear")} disabled={projectTaskState.busy}>Clear config</button>
                <button class="btn-subtle" onclick={() => runProjectTask("artisan_route_clear")} disabled={projectTaskState.busy}>Clear routes</button>
                <button class="btn-subtle" onclick={() => runProjectTask("artisan_view_clear")} disabled={projectTaskState.busy}>Clear views</button>
              </div>
            </div>
            {#if projectTaskState.busy}<div class="tool-note">{projectTaskState.operation || "Running..."}</div>{/if}
            {#if projectTaskOutput}<DetailsOutput title="Task output" value={projectTaskOutput} />{/if}
          {:else}
            <div class="tool-note">No artisan file detected — nothing to migrate or cache here.</div>
          {/if}
        </div>
      {:else if toolsTab === "packages"}
        <div class="tab-content details-panel">
          <div class="detail-section">
            <div class="project-files">
              {#if projectCapabilities.composerJson}<span>composer.json</span>{/if}
              {#if projectCapabilities.composerLock}<span>composer.lock</span>{/if}
              {#if projectCapabilities.packageJson}<span>package.json ({projectCapabilities.jsRunner})</span>{/if}
              {#if projectCapabilities.bundler}<span>{projectCapabilities.bundler}</span>{/if}
              {#if projectCapabilities.artisan}<span>artisan</span>{/if}
              {#if projectCapabilities.laravelEnv}<span>.env</span>{/if}
            </div>
          </div>
          {#if projectCapabilities.composerJson}
            <div class="detail-section">
              <div class="detail-section-title">Composer</div>
              <div class="project-task-row">
                <button class="btn-subtle" onclick={() => runProjectTask("composer_install")} disabled={projectTaskState.busy}>Composer install</button>
                <button class="btn-subtle" onclick={() => runProjectTask("composer_update")} disabled={projectTaskState.busy}>Composer update</button>
              </div>
            </div>
          {/if}
          {#if projectCapabilities.packageJson}
            <div class="detail-section">
              <div class="detail-section-title">{projectCapabilities.jsRunner}</div>
              <div class="project-task-row">
                <button class="btn-subtle" onclick={() => runProjectTask("npm_install")} disabled={projectTaskState.busy}>{projectCapabilities.jsRunner} install</button>
                <button class="btn-subtle" onclick={() => runProjectTask("npm_update")} disabled={projectTaskState.busy}>{projectCapabilities.jsRunner} update</button>
              </div>
              {#if !projectCapabilities.jsRunnerAvailable}<div class="tool-note">Falta el ejecutable {projectCapabilities.jsRunner}. Install it in DevPanel/bin first.</div>{/if}
            </div>
          {/if}
          {#if projectTaskState.busy}<div class="tool-note">{projectTaskState.operation || "Running..."}</div>{/if}
          {#if projectTaskOutput}<DetailsOutput title="Task output" value={projectTaskOutput} />{/if}
          {#if secondaryScripts(projectCapabilities).length}
            <div class="detail-section">
              <div class="detail-section-title">Composer scripts</div>
              <div class="project-task-row">
                {#each secondaryScripts(projectCapabilities) as scriptEntry (scriptEntry.name)}
                  {@const key = `composer:${scriptEntry.name}`}
                  {#if runningScripts.has(key)}
                    <button class="btn-subtle danger" onclick={() => stopProjectScript("composer", scriptEntry.name)} title={scriptEntry.command}>■ Stop {scriptEntry.name}</button>
                  {:else}
                    <button class="btn-subtle" onclick={() => startProjectScript("composer", scriptEntry.name)} disabled={!projectCapabilities.devpanelComposerAvailable} title={scriptEntry.command}>{scriptEntry.name}</button>
                  {/if}
                {/each}
              </div>
              {#each secondaryScripts(projectCapabilities) as scriptEntry (scriptEntry.name)}
                {@const key = `composer:${scriptEntry.name}`}
                {#if scriptOutputs[key]}<DetailsOutput title={`${scriptEntry.name} output`} value={scriptOutputs[key]} />{/if}
              {/each}
            </div>
          {/if}
          {#if noiseScripts(projectCapabilities).length}
            <details class="detail-section">
              <summary class="detail-section-title">Composer hook scripts ({noiseScripts(projectCapabilities).length})</summary>
              <div class="project-task-row">
                {#each noiseScripts(projectCapabilities) as scriptEntry (scriptEntry.name)}
                  {@const key = `composer:${scriptEntry.name}`}
                  {#if runningScripts.has(key)}
                    <button class="btn-subtle danger" onclick={() => stopProjectScript("composer", scriptEntry.name)} title={scriptEntry.command}>■ Stop {scriptEntry.name}</button>
                  {:else}
                    <button class="btn-subtle" onclick={() => startProjectScript("composer", scriptEntry.name)} disabled={!projectCapabilities.devpanelComposerAvailable} title={scriptEntry.command}>{scriptEntry.name}</button>
                  {/if}
                {/each}
              </div>
            </details>
          {/if}
        </div>
      {/if}
    {/if}
    </Dialog>
  {:else if detailPanel === "wordpress"}
    <Dialog
      title="WordPress Tools"
      width="min(520px, calc(100vw - 32px))"
      maxHeight="90%"
      scrollable
      onClose={() => (detailPanel = null)}
    >
      <WordPressSitePanel {workspace} onUpdated={onUpdated} />
    </Dialog>
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

