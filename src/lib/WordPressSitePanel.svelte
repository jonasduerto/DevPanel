<script>
  // @ts-nocheck -- Tauri command responses are runtime-shaped JSON.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import BusyButton from "#lib/BusyButton.svelte";
  import DetailsOutput from "#lib/DetailsOutput.svelte";
  import { invokeWith } from "#lib/tauri-utils.svelte.js";
  import { 
    RefreshCw, Shield, Zap, Database, Package, Layout, 
    AlertTriangle, CheckCircle, Clock, Trash2, Power,
    Activity, Wrench, Search
  } from "@lucide/svelte";

  let { workspace, onUpdated } = $props();

  let loading = $state(true);
  let version = $state(null);
  let tools = $state(null);
  let error = $state("");
  let repairResult = $state(null);
  let repairState = $state({ busy: false, error: "", operation: "" });

  // Plugin/Theme state
  let plugins = $state([]);
  let themes = $state([]);
  let loadingPlugins = $state(false);
  let loadingThemes = $state(false);

  // Performance & Security
  let siteHealth = $state(null);
  let performanceAnalysis = $state(null);
  let securityAudit = $state(null);
  let loadingHealth = $state(false);
  let loadingPerf = $state(false);
  let loadingSecurity = $state(false);

  // Search & Replace
  let searchOld = $state("");
  let searchNew = $state("");
  let searchResult = $state(null);

  // Active tab
  let activeTab = $state("overview");

  onMount(refresh);

  async function refresh() {
    loading = true;
    error = "";
    try {
      const [nextVersion, nextTools] = await Promise.all([
        invoke("get_wp_version", { id: workspace.id }).catch(() => null),
        invoke("get_wp_tool_status", { id: workspace.id }).catch(() => null),
      ]);
      version = nextVersion;
      tools = nextTools;
    } catch (e) {
      error = String(e);
    }
    loading = false;
  }

  async function repair() {
    if (repairState.busy) return;
    error = "";
    repairResult = null;
    await invokeWith(repairState, async () => {
      repairResult = await invoke("repair_workspace", { id: workspace.id });
      onUpdated?.();
      await refresh();
    }, "Repairing WordPress");
    error = repairState.error;
    if (repairResult) repairResult = JSON.stringify(repairResult, null, 2);
  }

  async function loadPlugins() {
    loadingPlugins = true;
    try {
      const result = await invoke("wp_plugin_list", { id: workspace.id });
      if (result.success) {
        plugins = result.plugins;
      } else {
        error = result.error || "Failed to load plugins";
      }
    } catch (e) {
      error = String(e);
    }
    loadingPlugins = false;
  }

  async function loadThemes() {
    loadingThemes = true;
    try {
      const result = await invoke("wp_theme_list", { id: workspace.id });
      if (result.success) {
        themes = result.themes;
      } else {
        error = result.error || "Failed to load themes";
      }
    } catch (e) {
      error = String(e);
    }
    loadingThemes = false;
  }

  async function togglePlugin(pluginName, currentStatus) {
    try {
      if (currentStatus === "active") {
        await invoke("wp_plugin_deactivate", { id: workspace.id, plugin: pluginName });
      } else {
        await invoke("wp_plugin_activate", { id: workspace.id, plugin: pluginName });
      }
      await loadPlugins();
    } catch (e) {
      error = String(e);
    }
  }

  async function deletePlugin(pluginName) {
    if (!confirm(`Delete plugin "${pluginName}"?`)) return;
    try {
      await invoke("wp_plugin_delete", { id: workspace.id, plugin: pluginName });
      await loadPlugins();
    } catch (e) {
      error = String(e);
    }
  }

  async function activateTheme(themeName) {
    try {
      await invoke("wp_theme_activate", { id: workspace.id, theme: themeName });
      await loadThemes();
    } catch (e) {
      error = String(e);
    }
  }

  async function updateCore() {
    if (!confirm("Update WordPress core to latest version?")) return;
    try {
      const result = await invoke("wp_core_update", { id: workspace.id });
      alert(result.success ? `Core updated:\n${result.output}` : `Error:\n${result.error}`);
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function reinstallCore() {
    if (!confirm("Reinstall WordPress core? This will replace core files but keep content.")) return;
    try {
      const result = await invoke("wp_core_reinstall", { id: workspace.id });
      alert(result.success ? `Core reinstalled:\n${result.output}` : `Error:\n${result.error}`);
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function updateAll() {
    if (!confirm("Update all plugins, themes, and WordPress core?")) return;
    try {
      const result = await invoke("wp_update_all", { id: workspace.id });
      alert(result.success ? `Updates complete:\n${result.output}` : `Error:\n${result.error}`);
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function flushCache() {
    try {
      const result = await invoke("wp_cache_flush", { id: workspace.id });
      alert(result.success ? "Cache flushed successfully!" : `Error: ${result.error}`);
    } catch (e) {
      error = String(e);
    }
  }

  async function cleanupTransients() {
    try {
      const result = await invoke("wp_transient_cleanup", { id: workspace.id });
      alert(result.success ? `Transients cleaned:\n${result.output}` : `Error: ${result.error}`);
    } catch (e) {
      error = String(e);
    }
  }

  async function runSearchReplace() {
    if (!searchOld || !searchNew) {
      alert("Please enter both old and new values");
      return;
    }
    if (!confirm(`Replace "${searchOld}" with "${searchNew}" in database?`)) return;
    try {
      searchResult = await invoke("wp_search_replace", { id: workspace.id, oldUrl: searchOld, newUrl: searchNew });
    } catch (e) {
      error = String(e);
    }
  }

  async function runSecurityAudit() {
    loadingSecurity = true;
    try {
      securityAudit = await invoke("wp_security_audit", { id: workspace.id });
    } catch (e) {
      error = String(e);
    }
    loadingSecurity = false;
  }

  async function applySecurityHardening() {
    if (!confirm("Apply security hardening? This will disable file editor and debug mode.")) return;
    try {
      const result = await invoke("wp_security_harden", { id: workspace.id });
      alert(result.success ? `Security hardening applied:\n${result.output}` : `Error:\n${result.error}`);
      await runSecurityAudit();
    } catch (e) {
      error = String(e);
    }
  }

  async function runSiteHealth() {
    loadingHealth = true;
    try {
      siteHealth = await invoke("wp_site_health", { id: workspace.id });
    } catch (e) {
      error = String(e);
    }
    loadingHealth = false;
  }

  async function runPerformanceAnalysis() {
    loadingPerf = true;
    try {
      performanceAnalysis = await invoke("wp_performance_analysis", { id: workspace.id });
    } catch (e) {
      error = String(e);
    }
    loadingPerf = false;
  }

  // Load data when switching tabs
  $effect(() => {
    if (activeTab === "plugins") loadPlugins();
    if (activeTab === "themes") loadThemes();
    if (activeTab === "health") runSiteHealth();
    if (activeTab === "performance") runPerformanceAnalysis();
    if (activeTab === "security") runSecurityAudit();
  });

</script>

<section class="wordpress-panel" aria-label="WordPress tools">
  <div class="heading">
    <div>
      <div class="title">WordPress</div>
      <div class="hint">{workspace.name}</div>
    </div>
    <span class="version">{loading ? "Checking…" : version ?? "Not detected"}</span>
  </div>

  <div class="tool-status">
    <span class:ready={tools?.php_found}>PHP</span>
    <span class:ready={tools?.wp_cli_found}>WP-CLI</span>
    <span class:ready={tools?.mysql_found}>MySQL</span>
  </div>

  <!-- Tab Navigation -->
  <div class="tab-nav">
    <button class:active={activeTab === "overview"} onclick={() => activeTab = "overview"}>
      <Wrench size={12} /> Overview
    </button>
    <button class:active={activeTab === "plugins"} onclick={() => activeTab = "plugins"}>
      <Package size={12} /> Plugins
    </button>
    <button class:active={activeTab === "themes"} onclick={() => activeTab = "themes"}>
      <Layout size={12} /> Themes
    </button>
    <button class:active={activeTab === "security"} onclick={() => activeTab = "security"}>
      <Shield size={12} /> Security
    </button>
    <button class:active={activeTab === "performance"} onclick={() => activeTab = "performance"}>
      <Activity size={12} /> Performance
    </button>
    <button class:active={activeTab === "database"} onclick={() => activeTab = "database"}>
      <Database size={12} /> Database
    </button>
  </div>

  <!-- Overview Tab -->
  {#if activeTab === "overview"}
    <div class="tab-content">
      <div class="action-grid">
        <BusyButton class="wp-btn primary" onclick={repair} disabled={loading} busy={repairState.busy}>
          <RefreshCw size={12} /> Repair WordPress
        </BusyButton>
        <button class="wp-btn" onclick={updateCore}>
          <RefreshCw size={12} /> Update Core
        </button>
        <button class="wp-btn" onclick={reinstallCore}>
          <RefreshCw size={12} /> Reinstall Core
        </button>
        <button class="wp-btn primary" onclick={updateAll}>
          <RefreshCw size={12} /> Update All
        </button>
        <button class="wp-btn" onclick={flushCache}>
          <Zap size={12} /> Flush Cache
        </button>
        <button class="wp-btn" onclick={cleanupTransients}>
          <Trash2 size={12} /> Clean Transients
        </button>
      </div>

      <div class="info-section">
        <h4>Quick Info</h4>
        <div class="info-grid">
          <div class="info-item">
            <span class="info-label">PHP</span>
            <span class="info-value" class:success={tools?.php_found}>{tools?.php_found ? "Available" : "Not found"}</span>
          </div>
          <div class="info-item">
            <span class="info-label">WP-CLI</span>
            <span class="info-value" class:success={tools?.wp_cli_found}>{tools?.wp_cli_found ? "Available" : "Not found"}</span>
          </div>
          <div class="info-item">
            <span class="info-label">MySQL</span>
            <span class="info-value" class:success={tools?.mysql_found}>{tools?.mysql_found ? "Available" : "Not found"}</span>
          </div>
        </div>
      </div>

      <div class="cli-note">WP-CLI is available. Use DevPanel terminal for advanced commands.</div>
    </div>
  {/if}

  <!-- Plugins Tab -->
  {#if activeTab === "plugins"}
    <div class="tab-content">
      <div class="section-header">
        <h4>WordPress Plugins</h4>
        <button class="wp-btn small" onclick={loadPlugins} disabled={loadingPlugins}>
          <RefreshCw size={10} /> Refresh
        </button>
      </div>

      {#if loadingPlugins}
        <div class="loading">Loading plugins...</div>
      {:else if plugins.length === 0}
        <div class="empty-state">No plugins found or WordPress not installed</div>
      {:else}
        <div class="plugin-list">
          {#each plugins as plugin}
            <div class="plugin-item">
              <div class="plugin-info">
                <span class="plugin-name">{plugin.name}</span>
                <span class="plugin-version">v{plugin.version}</span>
                {#if plugin.update_available}
                  <span class="update-badge">Update: {plugin.update_available}</span>
                {/if}
              </div>
              <div class="plugin-actions">
                <button 
                  class="wp-btn small" 
                  class:plugin-active={plugin.status === "active"}
                  onclick={() => togglePlugin(plugin.name, plugin.status)}
                >
                  <Power size={10} /> {plugin.status === "active" ? "Deactivate" : "Activate"}
                </button>
                {#if plugin.status !== "active"}
                  <button 
                    class="wp-btn small danger"
                    onclick={() => deletePlugin(plugin.name)}
                  >
                    <Trash2 size={10} /> Delete
                  </button>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <!-- Themes Tab -->
  {#if activeTab === "themes"}
    <div class="tab-content">
      <div class="section-header">
        <h4>WordPress Themes</h4>
        <button class="wp-btn small" onclick={loadThemes} disabled={loadingThemes}>
          <RefreshCw size={10} /> Refresh
        </button>
      </div>

      {#if loadingThemes}
        <div class="loading">Loading themes...</div>
      {:else if themes.length === 0}
        <div class="empty-state">No themes found</div>
      {:else}
        <div class="plugin-list">
          {#each themes as theme}
            <div class="plugin-item">
              <div class="plugin-info">
                <span class="plugin-name">{theme.name}</span>
                <span class="plugin-version">v{theme.version}</span>
                {#if theme.status === "active"}
                  <span class="active-badge">Active</span>
                {/if}
                {#if theme.update_available}
                  <span class="update-badge">Update: {theme.update_available}</span>
                {/if}
              </div>
              <div class="plugin-actions">
                {#if theme.status !== "active"}
                  <button 
                    class="wp-btn small"
                    onclick={() => activateTheme(theme.name)}
                  >
                    <Power size={10} /> Activate
                  </button>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <!-- Security Tab -->
  {#if activeTab === "security"}
    <div class="tab-content">
      <div class="section-header">
        <h4>Security Audit & Hardening</h4>
        <div class="header-actions">
          <button class="wp-btn small" onclick={runSecurityAudit} disabled={loadingSecurity}>
            <Search size={10} /> Audit
          </button>
          <button class="wp-btn small primary" onclick={applySecurityHardening}>
            <Shield size={10} /> Harden
          </button>
        </div>
      </div>

      {#if loadingSecurity}
        <div class="loading">Running security audit...</div>
      {:else if securityAudit?.checks}
        <div class="security-list">
          {#each securityAudit.checks as check}
            <div class="security-item">
              <div class="security-status security-{check.status}">
                {#if check.status === "pass"}
                  <CheckCircle size={14} />
                {:else if check.status === "warn"}
                  <AlertTriangle size={14} />
                {:else}
                  <AlertTriangle size={14} />
                {/if}
              </div>
              <div class="security-info">
                <span class="security-name">{check.name}</span>
                <span class="security-desc">{check.description}</span>
                {#if check.recommendation && check.status !== "pass"}
                  <span class="security-recommendation">{check.recommendation}</span>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="empty-state">Click "Audit" to check security status</div>
      {/if}
    </div>
  {/if}

  <!-- Performance Tab -->
  {#if activeTab === "performance"}
    <div class="tab-content">
      <div class="section-header">
        <h4>Performance Analysis & Site Health</h4>
        <div class="header-actions">
          <button class="wp-btn small" onclick={runSiteHealth} disabled={loadingHealth}>
            <Activity size={10} /> Health
          </button>
          <button class="wp-btn small" onclick={runPerformanceAnalysis} disabled={loadingPerf}>
            <Zap size={10} /> Analyze
          </button>
        </div>
      </div>

      {#if siteHealth}
        <div class="health-summary" class:good={siteHealth.health.status === "good"} class:warn={siteHealth.health.status === "recommended"} class:critical={siteHealth.health.status === "critical"}>
          <span class="health-score">Health Score: {siteHealth.health.score}/100</span>
          <span class="health-status">Status: {siteHealth.health.status.toUpperCase()}</span>
        </div>
      {/if}

      {#if loadingPerf}
        <div class="loading">Analyzing performance...</div>
      {:else if performanceAnalysis}
        <div class="metrics-grid">
          <div class="metric-card">
            <span class="metric-label">Posts</span>
            <span class="metric-value">{performanceAnalysis.metrics.post_count}</span>
          </div>
          <div class="metric-card">
            <span class="metric-label">Users</span>
            <span class="metric-value">{performanceAnalysis.metrics.user_count}</span>
          </div>
          <div class="metric-card">
            <span class="metric-label">Plugins</span>
            <span class="metric-value">{performanceAnalysis.metrics.plugin_count}</span>
          </div>
          <div class="metric-card">
            <span class="metric-label">Themes</span>
            <span class="metric-value">{performanceAnalysis.metrics.theme_count}</span>
          </div>
          <div class="metric-card">
            <span class="metric-label">Transients</span>
            <span class="metric-value">{performanceAnalysis.metrics.transient_count}</span>
          </div>
          <div class="metric-card">
            <span class="metric-label">Revisions</span>
            <span class="metric-value">{performanceAnalysis.metrics.revision_count}</span>
          </div>
        </div>

        {#if performanceAnalysis.bottlenecks.length > 0}
          <div class="issues-section">
            <h5>Performance Bottlenecks</h5>
            {#each performanceAnalysis.bottlenecks as bottleneck}
              <div class="issue-item warning">
                <AlertTriangle size={12} /> {bottleneck}
              </div>
            {/each}
          </div>
        {/if}

        {#if performanceAnalysis.recommendations.length > 0}
          <div class="issues-section">
            <h5>Recommendations</h5>
            {#each performanceAnalysis.recommendations as rec}
              <div class="issue-item info">
                <CheckCircle size={12} /> {rec}
              </div>
            {/each}
          </div>
        {/if}
      {:else}
        <div class="empty-state">Click "Analyze" to check performance</div>
      {/if}
    </div>
  {/if}

  <!-- Database Tab -->
  {#if activeTab === "database"}
    <div class="tab-content">
      <div class="section-header">
        <h4>Database Tools</h4>
      </div>

      <div class="search-replace">
        <h5>Search & Replace in Database</h5>
        <div class="sr-form">
          <input 
            type="text" 
            bind:value={searchOld} 
            placeholder="Old value (e.g., http://old-site.com)"
          />
          <span class="arrow">→</span>
          <input 
            type="text" 
            bind:value={searchNew} 
            placeholder="New value (e.g., http://new-site.com)"
          />
          <button class="wp-btn small primary" onclick={runSearchReplace}>
            <Search size={10} /> Replace
          </button>
        </div>
      </div>

      {#if searchResult}
        <div class="sr-result">
          <h5>Results</h5>
          <pre>{searchResult.output || searchResult.error}</pre>
        </div>
      {/if}

      <div class="db-actions">
        <button class="wp-btn" onclick={flushCache}>
          <Zap size={12} /> Flush All Cache
        </button>
        <button class="wp-btn" onclick={cleanupTransients}>
          <Trash2 size={12} /> Clean Expired Transients
        </button>
      </div>
    </div>
  {/if}

  {#if repairResult || error}
    <DetailsOutput title={error ? "Error" : "Results"} value={error || repairResult} />
  {/if}
</section>
