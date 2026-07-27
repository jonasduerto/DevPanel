<script>
  // @ts-nocheck
  import { invoke } from "@tauri-apps/api/core";
  import { openPath, openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import ConfirmDialog from "#lib/ConfirmDialog.svelte";
  import {
    Server, Database, Code2, Zap, Wrench, Mail,
    RefreshCw, TriangleAlert, Settings2, FileText,
    ExternalLink, Check, AlertCircle, Folder, ShieldCheck,
    SlidersHorizontal, PackageCheck, PackagePlus, Puzzle,
    KeyRound, UserPlus, Play, Square
  } from "@lucide/svelte";

  const categoryMeta = {
    WebServer: { label: "Web Servers", icon: Server },
    Database: { label: "Databases", icon: Database },
    Runtime: { label: "Runtimes", icon: Code2 },
    Cache: { label: "Caches", icon: Zap },
    Tool: { label: "Tools & Utilities", icon: Wrench },
  };

  const categoryOrder = ["WebServer", "Database", "Runtime", "Cache", "Tool"];

  /** @type {Array<{definition: {id: string, name: string, description: string, category: string, conflicts: string[], dashboard_capable: boolean}, state: {enabled: boolean, show_on_dashboard: boolean, selected_version: string | null}, available: boolean, running: boolean}>} */
  let modulesList = $state([]);
  let loading = $state(true);
  let error = $state("");
  let successMsg = $state("");
  /** @type {string[]} */
  let warnings = $state([]);

  // App Ports configuration
  let appPorts = $state({ apache: 80, nginx: 8080, mysql: 3306, postgres: 5432, redis: 6379 });
  let savingPorts = $state(false);
  let webPortConflict = $state(null);
  let resolvingWebPort = $state(false);

  // Per-module expanded settings state & loaded info
  /** @type {Record<string, boolean>} */
  let expandedSettings = $state({});
  /** @type {Record<string, {configPaths: any, logPaths: any, loading: boolean}>} */
  let moduleDetails = $state({});

  // PHP Extensions state (loaded inside PHP drawer)
  /** @type {any[]} */
  let phpExtensions = $state([]);
  let loadingExtensions = $state(false);
  let installingXdebug = $state(false);

  // Live process control (start/stop/restart) per module
  /** @type {Record<string, boolean>} */
  let actionBusy = $state({});
  let mailpitUrl = $state("http://127.0.0.1:8025");

  // MySQL-specific tools (root password, create DB+user, backups, version)
  let mysqlStatus = $state(null);
  let mysqlCurrentPassword = $state("");
  let mysqlNewPassword = $state("");
  let mysqlDbName = $state("");
  let mysqlDbUser = $state("");
  let mysqlDbPassword = $state("");
  /** @type {string[]} */
  let mysqlBackups = $state([]);
  let mysqlState = $state({ busy: false });
  let mysqlOutput = $state("");
  /** @type {string | null} */
  let mysqlPending = $state(null);

  onMount(async () => {
    await loadData();
  });

  async function loadData() {
    loading = true;
    error = "";
    try {
      const [addons, cfg, dbStatus, backups] = await Promise.all([
        invoke("list_addons"),
        invoke("get_config"),
        invoke("get_database_tool_status").catch(() => null),
        invoke("list_database_backups").catch(() => []),
      ]);
      modulesList = addons;
      if (cfg && cfg.ports) {
        appPorts = { ...cfg.ports };
      }
      mailpitUrl = cfg?.mailpit_url || "http://127.0.0.1:8025";
      mysqlStatus = dbStatus;
      mysqlBackups = backups;
    } catch (e) {
      error = String(e);
    }
    loading = false;
  }

  async function controlAddon(/** @type {any} */ addon, /** @type {"start" | "stop" | "restart"} */ action) {
    const id = addon.definition.id;
    actionBusy[id] = true;
    error = "";
    try {
      await invoke(`${action}_addon`, { addonId: id });
      await loadData();
    } catch (e) {
      error = String(e);
    }
    actionBusy[id] = false;
  }

  async function runMysqlTool(/** @type {string} */ action) {
    mysqlOutput = "";
    error = "";
    mysqlState.busy = true;
    try {
      let msg = "Operation completed";
      if (action === "root") {
        await invoke("update_mysql_root_password", { currentPassword: mysqlCurrentPassword, newPassword: mysqlNewPassword });
        mysqlCurrentPassword = "";
        mysqlNewPassword = "";
        msg = "MySQL root password updated";
      } else if (action === "user") {
        if (!mysqlDbName || !mysqlDbUser || !mysqlDbPassword) throw new Error("Fill in all 3 fields.");
        await invoke("create_database_and_user", { dbName: mysqlDbName, username: mysqlDbUser, password: mysqlDbPassword });
        mysqlDbName = "";
        mysqlDbUser = "";
        mysqlDbPassword = "";
        msg = "Database and user created";
      } else if (action === "backup") {
        const path = await invoke("backup_all_databases");
        msg = `Backed up at ${path}`;
      } else if (action === "repair") {
        const res = await invoke("repair_mysql_tables");
        msg = res.stdout || res.stderr || "Repair completed";
      } else if (action.startsWith("restore:")) {
        const backupName = action.slice(8);
        const res = await invoke("restore_database_backup", { backupName });
        msg = res.stdout || res.stderr || "Restore completed";
      }
      mysqlOutput = msg;
      successMsg = msg;
      setTimeout(() => (successMsg = ""), 4000);
      await loadData();
    } catch (e) {
      error = String(e);
    }
    mysqlState.busy = false;
  }

  async function changeMysqlVersion(/** @type {string} */ version) {
    error = "";
    mysqlState.busy = true;
    try {
      await invoke("set_mysql_version", { version: version || null });
      successMsg = "MySQL version updated";
      setTimeout(() => (successMsg = ""), 4000);
      await loadData();
    } catch (e) {
      error = String(e);
    }
    mysqlState.busy = false;
  }

  // Filter 1: Installed Modules (grouped by category)
  let installedCategories = $derived.by(() => {
    const groups = {};
    for (const a of modulesList) {
      if (a.available) {
        const cat = a.definition.category;
        if (!groups[cat]) groups[cat] = [];
        groups[cat].push(a);
      }
    }
    return groups;
  });

  // Filter 2: Not Installed Modules
  let uninstalledModules = $derived.by(() => {
    return modulesList.filter((a) => !a.available);
  });

  async function toggleEnabled(addon) {
    const newVal = !addon.state.enabled;
    if (newVal && isWebServer(addon.definition.id)) {
      const otherId = otherWebServer(addon.definition.id);
      const other = modulesList.find((item) => item.definition.id === otherId);
      if (other?.state.enabled && appPorts[addon.definition.id] === appPorts[otherId]) {
        webPortConflict = { targetId: addon.definition.id, otherId, addon };
        return;
      }
    }
    await setAddonEnabled(addon, newVal);
  }

  async function setAddonEnabled(addon, newVal) {
    warnings = [];
    error = "";
    try {
      const result = await invoke("enable_addon", { addonId: addon.definition.id, enabled: newVal });
      if (result && result.length > 0) {
        warnings = result.map((/** @type {any} */ w) => w.message);
        return;
      }
      addon.state.enabled = newVal;
      if (newVal && addon.definition.dashboard_capable && !addon.state.show_on_dashboard) {
        await invoke("set_addon_dashboard_visibility", { addonId: addon.definition.id, visible: true });
        addon.state.show_on_dashboard = true;
      }
      await loadData();
    } catch (e) {
      error = String(e);
    }
  }

  async function toggleDashboard(addon) {
    const newVal = !addon.state.show_on_dashboard;
    try {
      await invoke("set_addon_dashboard_visibility", { addonId: addon.definition.id, visible: newVal });
      addon.state.show_on_dashboard = newVal;
    } catch (e) {
      error = String(e);
    }
  }

  async function toggleModuleSettings(/** @type {string} */ moduleId) {
    expandedSettings[moduleId] = !expandedSettings[moduleId];
    if (expandedSettings[moduleId] && !moduleDetails[moduleId]) {
      moduleDetails[moduleId] = { configPaths: null, logPaths: null, loading: true };
      try {
        const [configPaths, logPaths] = await Promise.all([
          invoke("get_service_config_paths", { id: moduleId }).catch(() => ({ main_config: null, extra_configs: [] })),
          invoke("get_service_log_paths", { id: moduleId }).catch(() => ({ error_log: null, access_log: null })),
        ]);
        moduleDetails[moduleId] = { configPaths, logPaths, loading: false };
      } catch (e) {
        moduleDetails[moduleId] = { configPaths: null, logPaths: null, loading: false };
      }

      if (moduleId === "php") {
        await loadPhpExtensions();
      }
    }
  }

  async function loadPhpExtensions() {
    loadingExtensions = true;
    try {
      phpExtensions = await invoke("get_php_extensions");
    } catch (e) {
      error = `PHP extensions error: ${e}`;
    }
    loadingExtensions = false;
  }

  async function togglePhpExtension(ext) {
    try {
      await invoke("set_php_extension", { fileName: ext.file_name, enabled: !ext.enabled });
      ext.enabled = !ext.enabled;
      successMsg = `PHP extension ${ext.name} ${ext.enabled ? "enabled" : "disabled"}`;
      setTimeout(() => (successMsg = ""), 3000);
    } catch (e) {
      error = String(e);
    }
  }

  async function installXdebug() {
    installingXdebug = true;
    error = "";
    try {
      await invoke("install_xdebug");
      await loadPhpExtensions();
      successMsg = "Xdebug installed successfully!";
      setTimeout(() => (successMsg = ""), 4000);
    } catch (e) {
      error = String(e);
    }
    installingXdebug = false;
  }

  async function openFilePath(/** @type {string} */ path) {
    try {
      await openPath(path);
    } catch (e) {
      error = `Unable to open path: ${e}`;
    }
  }

  async function savePortConfig(moduleId = null) {
    if (isWebServer(moduleId) && appPorts.apache === appPorts.nginx) {
      webPortConflict = { targetId: moduleId, otherId: otherWebServer(moduleId), addon: null };
      return;
    }
    await persistPorts();
  }

  async function persistPorts() {
    savingPorts = true;
    error = "";
    successMsg = "";
    try {
      const portWarnings = await invoke("set_ports", { ports: appPorts });
      if (portWarnings && portWarnings.length > 0) {
        warnings = portWarnings;
      }
      successMsg = "Module ports saved successfully!";
      setTimeout(() => (successMsg = ""), 4000);
    } catch (e) {
      error = String(e);
    }
    savingPorts = false;
  }

  function isWebServer(id) {
    return id === "apache" || id === "nginx";
  }

  function otherWebServer(id) {
    return id === "apache" ? "nginx" : "apache";
  }

  function displayModuleName(id) {
    return id === "apache" ? "Apache" : "Nginx";
  }

  async function resolveWebPortConflict() {
    if (!webPortConflict) return;
    resolvingWebPort = true;
    error = "";
    try {
      const targetId = webPortConflict.targetId;
      const suggestedPort = await invoke("suggest_available_web_port", {
        preferredPort: 8080,
        reservedPort: appPorts[webPortConflict.otherId],
      });
      appPorts[targetId] = suggestedPort;
      const addonToEnable = webPortConflict.addon;
      webPortConflict = null;
      await persistPorts();
      if (addonToEnable) {
        await setAddonEnabled(addonToEnable, true);
      }
    } catch (e) {
      error = String(e);
    }
    resolvingWebPort = false;
  }

  async function testConfig(/** @type {string} */ moduleId) {
    error = "";
    successMsg = "";
    try {
      const res = await invoke("test_service_config", { id: moduleId });
      if (res.success) {
        successMsg = `${moduleId.toUpperCase()} configuration check passed! ${res.stdout || ""}`;
      } else {
        error = `${moduleId.toUpperCase()} config test failed: ${res.stderr || "Check log files."}`;
      }
      setTimeout(() => { successMsg = ""; }, 5000);
    } catch (e) {
      error = String(e);
    }
  }

  async function gracefulRestart(/** @type {string} */ moduleId) {
    error = "";
    successMsg = "";
    try {
      const res = await invoke("graceful_restart_service", { id: moduleId });
      successMsg = res.stdout || res.stderr || `${moduleId} restarted gracefully`;
      setTimeout(() => { successMsg = ""; }, 5000);
      await loadData();
    } catch (e) {
      error = String(e);
    }
  }

  function getModulePortKey(id) {
    if (id === "apache" || id === "nginx") return id;
    if (id === "mysql") return "mysql";
    if (id === "postgres") return "postgres";
    if (id === "redis") return "redis";
    return null;
  }

  function getInstallGuidance(id) {
    switch (id) {
      case "apache":
        return { path: "bin/apache/httpd.exe", tip: "Place Apache HTTPD binary package in bin/apache/" };
      case "nginx":
        return { path: "bin/nginx/nginx.exe", tip: "Place Nginx server binary package in bin/nginx/" };
      case "mysql":
        return { path: "bin/mysql/mysqld.exe", tip: "Place MySQL / MariaDB binaries in bin/mysql/" };
      case "postgres":
        return { path: "bin/postgres/postgres.exe", tip: "Place PostgreSQL binaries in bin/postgres/" };
      case "php":
        return { path: "bin/php/php.exe", tip: "Place PHP runtime package in bin/php/" };
      case "node":
        return { path: "bin/node/node.exe", tip: "Place Node.js executable in bin/node/" };
      case "redis":
        return { path: "bin/redis/redis-server.exe", tip: "Place Redis server binary in bin/redis/" };
      case "mailpit":
        return { path: "bin/sendmail/mailpit.exe", tip: "Place Mailpit executable in bin/sendmail/" };
      default:
        return { path: `bin/${id}/`, tip: `Place ${id} binaries into bin/${id}/` };
    }
  }
</script>

<div class="modules-view">
  <div class="modules-header">
    <div class="header-main">
      <h2 class="view-title">Environment Modules & Customization</h2>
      <p class="view-desc">Enable, configure, and customize installed modules and settings (runtimes, ports, php.ini, extensions, and logs).</p>
    </div>
    <button class="btn-ghost btn-sm" onclick={loadData} disabled={loading}>
      <RefreshCw size={13} class={loading ? "spin" : ""} />
      <span>Refresh Status</span>
    </button>
  </div>

  {#if successMsg}
    <div class="success-banner">
      <Check size={14} />
      <span>{successMsg}</span>
    </div>
  {/if}

  {#if warnings.length > 0}
    <div class="warnings-banner">
      <TriangleAlert size={14} />
      <div>
        {#each warnings as w}
          <div>{w}</div>
        {/each}
      </div>
    </div>
  {/if}

  {#if error}
    <div class="error-banner">
      <AlertCircle size={14} />
      <span>{error}</span>
    </div>
  {/if}

  {#if webPortConflict}
    <div class="modal-overlay" role="presentation">
      <dialog open class="modal-dialog" aria-labelledby="web-port-conflict-title">
        <div class="modal-header">
          <h3 id="web-port-conflict-title">Web server port conflict</h3>
        </div>
        <div class="modal-body">
          <p>
            {displayModuleName(webPortConflict.otherId)} already uses port {appPorts[webPortConflict.otherId]}.
            {displayModuleName(webPortConflict.targetId)} cannot use the same port while both are active.
          </p>
          <p>Use the next available port, starting at 8080, for {displayModuleName(webPortConflict.targetId)}?</p>
        </div>
        <div class="modal-footer">
          <button class="btn-secondary" onclick={() => (webPortConflict = null)} disabled={resolvingWebPort}>Cancel</button>
          <button class="btn-primary" onclick={resolveWebPortConflict} disabled={resolvingWebPort}>
            {resolvingWebPort ? "Finding free port..." : "Use available port"}
          </button>
        </div>
      </dialog>
    </div>
  {/if}

  {#if loading}
    <div class="loading-state">
      <RefreshCw size={24} class="spin text-accent" />
      <p>Loading modules & environment configurations...</p>
    </div>
  {:else}
    <!-- SECTION 1: INSTALLED MODULES -->
    <div class="section-title-wrapper">
      <PackageCheck size={16} class="text-accent" />
      <h3 class="group-section-title">Installed Modules</h3>
    </div>

    {#each categoryOrder as cat}
      {#if installedCategories[cat]?.length}
        <section class="module-group">
          <div class="group-header">
            <div class="cat-icon" role="img" aria-label={categoryMeta[cat]?.label ?? cat}>
              {#if cat === "WebServer"}
                <Server size={16} />
              {:else if cat === "Database"}
                <Database size={16} />
              {:else if cat === "Runtime"}
                <Code2 size={16} />
              {:else if cat === "Cache"}
                <Zap size={16} />
              {:else if cat === "Tool"}
                <Wrench size={16} />
              {/if}
            </div>
            <h4 class="group-title">{categoryMeta[cat]?.label ?? cat}</h4>
            <span class="group-count">{installedCategories[cat].length}</span>
          </div>

          <div class="module-cards">
            {#each installedCategories[cat] as addon (addon.definition.id)}
              {@const portKey = getModulePortKey(addon.definition.id)}
              <div class="module-card available" class:enabled={addon.state.enabled}>
                <div class="card-top">
                  <div class="card-info">
                    <div class="card-name-row">
                      <h4 class="module-name">{addon.definition.name}</h4>
                      <span class="pill-badge success">Installed</span>
                      {#if addon.state.enabled}
                        <span class="pill-badge accent">Activated</span>
                        <span class="pill-badge" class:success={addon.running}>{addon.running ? "Running" : "Stopped"}</span>
                      {:else}
                        <span class="pill-badge">Disabled</span>
                      {/if}
                    </div>
                    <p class="module-desc">{addon.definition.description}</p>
                  </div>
                </div>

                <div class="card-controls">
                  <label class="toggle-switch">
                    <input
                      type="checkbox"
                      checked={addon.state.enabled}
                      onchange={() => toggleEnabled(addon)}
                    />
                    <span class="toggle-label">Enable Module</span>
                  </label>

                  {#if addon.state.enabled}
                    <div class="buttons-row">
                      <button class="btn-secondary btn-xs" onclick={() => controlAddon(addon, "start")} disabled={actionBusy[addon.definition.id] || addon.running}>
                        <Play size={12} />
                        <span>Start</span>
                      </button>
                      <button class="btn-secondary btn-xs" onclick={() => controlAddon(addon, "restart")} disabled={actionBusy[addon.definition.id]}>
                        <RefreshCw size={12} />
                        <span>Restart</span>
                      </button>
                      <button class="btn-secondary btn-xs" onclick={() => controlAddon(addon, "stop")} disabled={actionBusy[addon.definition.id] || !addon.running}>
                        <Square size={12} />
                        <span>Stop</span>
                      </button>
                    </div>
                  {/if}

                  {#if addon.definition.dashboard_capable && addon.state.enabled}
                    <label class="toggle-switch">
                      <input
                        type="checkbox"
                        checked={addon.state.show_on_dashboard}
                        onchange={() => toggleDashboard(addon)}
                      />
                      <span class="toggle-label">Show on Dashboard</span>
                    </label>
                  {/if}

                  <button
                    class="btn-settings-toggle"
                    class:active={expandedSettings[addon.definition.id]}
                    onclick={() => toggleModuleSettings(addon.definition.id)}
                  >
                    <SlidersHorizontal size={13} />
                    <span>{expandedSettings[addon.definition.id] ? "Hide Settings" : "Configure & Options"}</span>
                  </button>
                </div>

                <!-- Expandable Settings & Customization Panel -->
                {#if expandedSettings[addon.definition.id]}
                  <div class="settings-drawer">
                    <div class="drawer-header">
                      <Settings2 size={14} class="text-accent" />
                      <span>{addon.definition.name} Customization & Options</span>
                    </div>

                    <div class="drawer-grid">
                      <!-- Network Port Customization -->
                      {#if portKey}
                        <div class="setting-item">
                          <span class="setting-label">Bind Port</span>
                          <div class="port-input-group">
                            <input
                              type="number"
                              class="port-input"
                              bind:value={appPorts[portKey]}
                              placeholder="Port"
                            />
                            <button
                              class="btn-primary btn-xs"
                              onclick={() => savePortConfig(addon.definition.id)}
                              disabled={savingPorts}
                            >
                              Save Port
                            </button>
                          </div>
                          <span class="setting-hint">
                            {isWebServer(addon.definition.id)
                              ? "Apache and Nginx can run together when their Bind Ports are different."
                              : `Target port for ${addon.definition.name} connections`}
                          </span>
                        </div>
                      {/if}

                      <!-- Configuration Files -->
                      <div class="setting-item">
                        <span class="setting-label">Configuration Files</span>
                        <div class="buttons-row">
                          {#if moduleDetails[addon.definition.id]?.configPaths?.main_config}
                            <button
                              class="btn-secondary btn-xs"
                              onclick={() => openFilePath(moduleDetails[addon.definition.id].configPaths.main_config)}
                            >
                              <FileText size={12} />
                              <span>Open Main Config</span>
                            </button>
                          {/if}
                          {#each moduleDetails[addon.definition.id]?.configPaths?.extra_configs ?? [] as extraPath}
                            <button
                              class="btn-secondary btn-xs"
                              onclick={() => openFilePath(extraPath)}
                            >
                              <FileText size={12} />
                              <span>{extraPath.split(/[\\/]/).pop()}</span>
                            </button>
                          {/each}
                          {#if !moduleDetails[addon.definition.id]?.configPaths?.main_config}
                            <span class="no-files-hint">No direct config file found</span>
                          {/if}
                        </div>
                      </div>

                      <!-- Log Files -->
                      <div class="setting-item">
                        <span class="setting-label">Log Files</span>
                        <div class="buttons-row">
                          {#if moduleDetails[addon.definition.id]?.logPaths?.error_log}
                            <button
                              class="btn-secondary btn-xs"
                              onclick={() => openFilePath(moduleDetails[addon.definition.id].logPaths.error_log)}
                            >
                              <FileText size={12} />
                              <span>Error Log</span>
                            </button>
                          {/if}
                          {#if moduleDetails[addon.definition.id]?.logPaths?.access_log}
                            <button
                              class="btn-secondary btn-xs"
                              onclick={() => openFilePath(moduleDetails[addon.definition.id].logPaths.access_log)}
                            >
                              <FileText size={12} />
                              <span>Access Log</span>
                            </button>
                          {/if}
                          {#if !moduleDetails[addon.definition.id]?.logPaths?.error_log && !moduleDetails[addon.definition.id]?.logPaths?.access_log}
                            <span class="no-files-hint">No log files registered</span>
                          {/if}
                        </div>
                      </div>

                      <!-- PHP Extensions Manager (Inside PHP Module Drawer) -->
                      {#if addon.definition.id === "php"}
                        <div class="setting-item full-width">
                          <span class="setting-label">PHP Extensions & Xdebug</span>
                          <div class="extensions-wrapper">
                            {#if loadingExtensions}
                              <div class="hint">Loading PHP extensions...</div>
                            {:else}
                              <div class="extensions-grid">
                                {#each phpExtensions as ext}
                                  <label class="ext-checkbox">
                                    <input
                                      type="checkbox"
                                      checked={ext.enabled}
                                      onchange={() => togglePhpExtension(ext)}
                                    />
                                    <span class="ext-name">{ext.name}</span>
                                  </label>
                                {/each}
                              </div>
                              <div class="xdebug-row">
                                <button
                                  class="btn-secondary btn-xs"
                                  disabled={installingXdebug}
                                  onclick={installXdebug}
                                >
                                  <Puzzle size={12} />
                                  <span>{installingXdebug ? "Installing Xdebug..." : "Install Xdebug via PIE"}</span>
                                </button>
                              </div>
                            {/if}
                          </div>
                        </div>
                      {/if}

                      <!-- Config Verification Actions -->
                      {#if ["apache", "nginx", "php", "mysql"].includes(addon.definition.id)}
                        <div class="setting-item">
                          <span class="setting-label">Validation & Tools</span>
                          <div class="buttons-row">
                            <button
                              class="btn-secondary btn-xs"
                              onclick={() => testConfig(addon.definition.id)}
                            >
                              <ShieldCheck size={12} />
                              <span>Test Config Syntax</span>
                            </button>
                            {#if ["apache", "nginx"].includes(addon.definition.id)}
                              <button
                                class="btn-secondary btn-xs"
                                onclick={() => gracefulRestart(addon.definition.id)}
                              >
                                <RefreshCw size={12} />
                                <span>Graceful Restart</span>
                              </button>
                            {/if}
                          </div>
                        </div>
                      {/if}

                      {#if addon.definition.id === "mailpit"}
                        <div class="setting-item">
                          <span class="setting-label">Web Interface</span>
                          <div class="buttons-row">
                            <button
                              class="btn-secondary btn-xs"
                              onclick={() => openUrl(mailpitUrl)}
                            >
                              <ExternalLink size={12} />
                              <span>Open Web GUI</span>
                            </button>
                          </div>
                        </div>
                      {/if}

                      <!-- MySQL-specific management tools -->
                      {#if addon.definition.id === "mysql"}
                        <div class="setting-item full-width">
                          <span class="setting-label">MySQL Version</span>
                          <div class="buttons-row">
                            <select
                              value={mysqlStatus?.selected_version ?? ""}
                              onchange={(event) => changeMysqlVersion(event.currentTarget.value)}
                              disabled={mysqlState.busy || mysqlStatus?.mysql_running}
                            >
                              <option value="">Most recent detected</option>
                              {#each mysqlStatus?.mysql_versions ?? [] as version (version)}
                                <option value={version}>{version}</option>
                              {/each}
                            </select>
                          </div>
                          {#if mysqlStatus?.mysql_running}
                            <span class="setting-hint">Stop MySQL before switching versions.</span>
                          {/if}
                        </div>

                        <div class="setting-item full-width">
                          <span class="setting-label">Root Password</span>
                          <div class="buttons-row">
                            <input type="password" class="port-input" style="width: 140px" bind:value={mysqlCurrentPassword} autocomplete="current-password" placeholder="Current password" />
                            <input type="password" class="port-input" style="width: 140px" bind:value={mysqlNewPassword} autocomplete="new-password" placeholder="New password" />
                            <button class="btn-secondary btn-xs" onclick={() => (mysqlPending = "root")} disabled={mysqlState.busy}>
                              <KeyRound size={12} />
                              <span>Update</span>
                            </button>
                          </div>
                        </div>

                        <div class="setting-item full-width">
                          <span class="setting-label">Create Database + User</span>
                          <div class="buttons-row">
                            <input class="port-input" style="width: 120px" bind:value={mysqlDbName} placeholder="DB Name" />
                            <input class="port-input" style="width: 120px" bind:value={mysqlDbUser} placeholder="User" />
                            <input type="password" class="port-input" style="width: 120px" bind:value={mysqlDbPassword} autocomplete="new-password" placeholder="Password" />
                            <button class="btn-secondary btn-xs" onclick={() => runMysqlTool("user")} disabled={mysqlState.busy}>
                              <UserPlus size={12} />
                              <span>Create</span>
                            </button>
                          </div>
                        </div>

                        <div class="setting-item full-width">
                          <span class="setting-label">Backups & Repair</span>
                          <div class="buttons-row">
                            <button class="btn-secondary btn-xs" onclick={() => runMysqlTool("backup")} disabled={mysqlState.busy}>
                              <PackagePlus size={12} />
                              <span>Backup all</span>
                            </button>
                            <button class="btn-secondary btn-xs" onclick={() => (mysqlPending = "repair")} disabled={mysqlState.busy}>
                              <Wrench size={12} />
                              <span>Repair tables</span>
                            </button>
                          </div>
                          {#if mysqlBackups.length}
                            <div class="buttons-row" style="flex-wrap: wrap; margin-top: 6px;">
                              {#each mysqlBackups as backup (backup)}
                                <button class="btn-secondary btn-xs" onclick={() => (mysqlPending = `restore:${backup}`)} disabled={mysqlState.busy}>
                                  Restore {backup}
                                </button>
                              {/each}
                            </div>
                          {:else}
                            <span class="setting-hint">No backups generated yet.</span>
                          {/if}
                        </div>
                        {#if mysqlOutput}
                          <div class="setting-item full-width">
                            <span class="no-files-hint">{mysqlOutput}</span>
                          </div>
                        {/if}
                      {/if}
                    </div>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        </section>
      {/if}
    {/each}

    <!-- SECTION 2: NOT INSTALLED / AVAILABLE FOR SETUP MODULES -->
    {#if uninstalledModules.length > 0}
      <div class="section-title-wrapper mt-6">
        <PackagePlus size={16} class="text-warning" />
        <h3 class="group-section-title">Available / Not Installed Modules</h3>
      </div>

      <div class="uninstalled-grid">
        {#each uninstalledModules as addon (addon.definition.id)}
          {@const installInfo = getInstallGuidance(addon.definition.id)}
          <div class="module-card uninstalled">
            <div class="card-top">
              <div class="card-info">
                <div class="card-name-row">
                  <h4 class="module-name">{addon.definition.name}</h4>
                  <span class="pill-badge warning">Not installed</span>
                </div>
                <p class="module-desc">{addon.definition.description}</p>
              </div>
            </div>

            <div class="installation-box">
              <div class="install-header">
                <Folder size={13} />
                <span>Binary Installation Folder:</span>
              </div>
              <code class="install-path">{installInfo.path}</code>
              <p class="install-tip">{installInfo.tip}</p>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

{#if mysqlPending}
  <ConfirmDialog
    message={mysqlPending === "root" ? "Change MySQL root password?" : mysqlPending === "repair" ? "Repair all MySQL tables?" : "Restore backup " + mysqlPending.slice(8) + "?"}
    confirmLabel={mysqlPending.startsWith("restore:") ? "Restore" : mysqlPending === "repair" ? "Repair" : "Update"}
    onCancel={() => (mysqlPending = null)}
    onConfirm={() => { const a = mysqlPending; mysqlPending = null; runMysqlTool(a); }}
  />
{/if}

