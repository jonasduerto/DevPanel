<script>
  // @ts-nocheck
  import { invoke } from "@tauri-apps/api/core";
  import { openPath, openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import ConfirmDialog from "#lib/ConfirmDialog.svelte";
  import DetailsOutput from "#lib/DetailsOutput.svelte";
  import {
    Bug, Shield, Database, FileText, Network, Mail,
    Check, RefreshCw, ExternalLink, FolderOpen, KeyRound
  } from "@lucide/svelte";

  const categories = [
    { id: "xdebug", label: "Xdebug & Profiler", icon: Bug },
    { id: "ssl", label: "SSL & Hosts", icon: Shield },
    { id: "database-gui", label: "Database GUI", icon: Database },
    { id: "logs", label: "Log Viewer", icon: FileText },
    { id: "ports", label: "Port Inspector", icon: Network },
    { id: "mail", label: "Mailpit", icon: Mail },
  ];

  const MAILPIT_URL = "http://127.0.0.1:8025";
  const XDEBUG_MODES = ["off", "debug", "profile", "trace"];
  const LOG_SERVICE_IDS = ["apache", "nginx", "php", "mysql"];

  let activeCategory = $state("xdebug");
  let output = $state("");
  let toolState = $state({ busy: false });

  // Xdebug & Profiler
  let xdebugEnabled = $state(false);
  let xdebugMode = $state("off");
  /** @type {Array<{name: string, size_bytes: number, modified_unix: number}>} */
  let xdebugFiles = $state([]);

  // SSL & Hosts
  let caTrusted = $state(false);
  let tld = $state(".test");
  /** @type {string[]} */
  let hostsEntries = $state([]);
  const TLDS = [".dp", ".dev", ".local", ".test"];

  // Database GUI
  /** @type {Array<{id: string, label: string, url: string}>} */
  let webApps = $state([]);

  // Log Viewer
  /** @type {Record<string, string>} */
  let logs = $state({});

  // Port Inspector
  /** @type {Array<{port: number, label: string, pid: number | null, process_name: string | null}>} */
  let ports = $state([]);
  /** @type {number | null} */
  let pendingKillPid = $state(null);

  onMount(refresh);

  async function refresh() {
    output = "";
    try {
      if (activeCategory === "xdebug") await refreshXdebug();
      else if (activeCategory === "ssl") await refreshSsl();
      else if (activeCategory === "database-gui") await refreshWebApps();
      else if (activeCategory === "logs") await refreshLogs();
      else if (activeCategory === "ports") await refreshPorts();
    } catch (e) {
      output = String(e);
    }
  }

  function selectCategory(id) {
    activeCategory = id;
    refresh();
  }

  async function refreshXdebug() {
    const [extensions, mode] = await Promise.all([
      invoke("get_php_extensions").catch(() => []),
      invoke("get_xdebug_mode").catch(() => "off"),
    ]);
    xdebugEnabled = extensions.some((ext) => ext.name.toLowerCase() === "xdebug" && ext.enabled);
    xdebugMode = mode;
    xdebugFiles = await invoke("list_xdebug_output").catch(() => []);
  }

  async function toggleXdebug() {
    await invokeWith(async () => {
      const extensions = await invoke("get_php_extensions");
      const xdebug = extensions.find((ext) => ext.name.toLowerCase() === "xdebug");
      if (!xdebug) throw new Error("Xdebug is not installed. Install it from Environment → Modules → PHP.");
      await invoke("set_php_extension", { fileName: xdebug.file_name, enabled: !xdebug.enabled });
      await refreshXdebug();
    }, "Toggling Xdebug");
  }

  async function changeXdebugMode(mode) {
    await invokeWith(async () => {
      await invoke("set_xdebug_mode", { mode });
      await refreshXdebug();
    }, "Changing Xdebug mode");
  }

  async function openXdebugFolder() {
    try {
      const path = await invoke("open_xdebug_output_folder");
      await openPath(path);
    } catch (e) {
      output = String(e);
    }
  }

  async function refreshSsl() {
    const [trusted, config, entries] = await Promise.all([
      invoke("get_ca_trusted"),
      invoke("get_config"),
      invoke("list_devpanel_hosts_entries").catch(() => []),
    ]);
    caTrusted = trusted;
    tld = config.tld;
    hostsEntries = entries;
  }

  async function trustCA() {
    await invokeWith(async () => {
      await invoke("trust_local_ca");
      await refreshSsl();
    }, "Trusting Root CA");
  }

  async function changeTld(newTld) {
    if (newTld === tld) return;
    await invokeWith(async () => {
      const warnings = await invoke("set_tld", { tld: newTld });
      if (warnings.length) output = warnings.join("\n");
      await refreshSsl();
    }, "Changing TLD");
  }

  async function removeHostsEntry(domain) {
    await invokeWith(async () => {
      await invoke("remove_hosts_entry", { domain });
      await refreshSsl();
    }, `Removing ${domain}`);
  }

  async function refreshWebApps() {
    webApps = await invoke("list_installed_web_apps").catch(() => []);
  }

  async function openHeidiSQL() {
    try {
      await invoke("launch_heidisql");
    } catch (e) {
      output = String(e);
    }
  }

  async function refreshLogs() {
    const results = await Promise.all(
      LOG_SERVICE_IDS.map((id) => invoke("read_service_log", { id, maxLines: 80 }).catch((e) => String(e)))
    );
    logs = Object.fromEntries(LOG_SERVICE_IDS.map((id, i) => [id, results[i]]));
  }

  async function refreshPorts() {
    ports = await invoke("list_known_ports").catch(() => []);
  }

  async function killPid(pid) {
    await invokeWith(async () => {
      await invoke("kill_process", { pid });
      await refreshPorts();
    }, `Stopping process ${pid}`);
  }

  async function invokeWith(fn, label) {
    output = "";
    toolState = { busy: true };
    try {
      await fn();
    } catch (e) {
      output = String(e);
    }
    toolState = { busy: false };
  }
</script>

<div class="tools-view">
  <div class="tools-sidebar">
    {#each categories as cat (cat.id)}
      <button
        class="cat-btn"
        class:active={activeCategory === cat.id}
        onclick={() => selectCategory(cat.id)}
      >
        <cat.icon size={14} />
        <span>{cat.label}</span>
      </button>
    {/each}
  </div>

  <div class="tools-content">
    {#if activeCategory === "xdebug"}
      <section class="tool-card">
        <div class="card-header-row">
          <Bug size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">Xdebug & Profiler</h3>
            <p class="card-desc">Toggle Xdebug and switch its mode for the shared PHP runtime.</p>
          </div>
          <span class="status-tag" class:running={xdebugEnabled}>{xdebugEnabled ? "Enabled" : "Disabled"}</span>
        </div>

        <div class="sub-section">
          <h4 class="sub-title">Xdebug</h4>
          <div class="sub-body">
            <button class="btn-secondary" onclick={toggleXdebug} disabled={toolState.busy}>
              <span>{xdebugEnabled ? "Disable Xdebug" : "Enable Xdebug"}</span>
            </button>
            <div class="hint">Extension list and Xdebug installation live in Environment &rarr; Modules &rarr; PHP.</div>
          </div>
        </div>

        <div class="sub-section">
          <h4 class="sub-title">Mode</h4>
          <div class="sub-body">
            <select value={xdebugMode} onchange={(event) => changeXdebugMode(event.currentTarget.value)} disabled={toolState.busy || !xdebugEnabled}>
              {#each XDEBUG_MODES as mode (mode)}<option value={mode}>{mode}</option>{/each}
            </select>
            {#if !xdebugEnabled}<div class="hint">Enable Xdebug before choosing a mode.</div>{/if}
          </div>
        </div>

        <div class="sub-section">
          <h4 class="sub-title">Profiler / Trace Output</h4>
          <div class="sub-body">
            <button class="btn-secondary" onclick={openXdebugFolder}>
              <FolderOpen size={12} />
              <span>Open output folder</span>
            </button>
            {#if xdebugFiles.length}
              <div class="backup-list">
                {#each xdebugFiles as file (file.name)}
                  <div class="backup-item">
                    <code>{file.name}</code>
                    <span class="server-status">{(file.size_bytes / 1024).toFixed(1)} KB</span>
                  </div>
                {/each}
              </div>
            {:else}
              <div class="hint">No profiler or trace files yet.</div>
            {/if}
          </div>
        </div>
      </section>

    {:else if activeCategory === "ssl"}
      <section class="tool-card">
        <div class="card-header-row">
          <Shield size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">SSL & Hosts</h3>
            <p class="card-desc">Local Certificate Authority trust, domain suffix, and hosts-file cleanup.</p>
          </div>
        </div>

        <div class="sub-section">
          <h4 class="sub-title">Root Certificate</h4>
          <div class="sub-body">
            <div class="action-row">
              <span class="status-tag" class:running={caTrusted}>{caTrusted ? "Trusted in Windows" : "Not trusted yet"}</span>
              <button class="btn-secondary" disabled={caTrusted || toolState.busy} onclick={trustCA}>
                <Check size={12} />
                <span>{caTrusted ? "CA Certificate Installed" : "Trust Root CA"}</span>
              </button>
            </div>
          </div>
        </div>

        <div class="sub-section">
          <h4 class="sub-title">Local Domain Suffix (TLD)</h4>
          <div class="sub-body">
            <div class="action-row">
              {#each TLDS as t (t)}
                <button class="btn-secondary btn-xs" class:active={tld === t} disabled={toolState.busy} onclick={() => changeTld(t)}>{t}</button>
              {/each}
            </div>
          </div>
        </div>

        <div class="sub-section">
          <h4 class="sub-title">Hosts Entries Managed by DevPanel</h4>
          <div class="sub-body">
            {#if hostsEntries.length}
              <div class="backup-list">
                {#each hostsEntries as domain (domain)}
                  <div class="backup-item">
                    <code>{domain}</code>
                    <button class="danger-link" onclick={() => removeHostsEntry(domain)} disabled={toolState.busy}>Remove</button>
                  </div>
                {/each}
              </div>
            {:else}
              <div class="hint">No hosts entries found.</div>
            {/if}
          </div>
        </div>
      </section>

    {:else if activeCategory === "database-gui"}
      <section class="tool-card">
        <div class="card-header-row">
          <Database size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">Database GUI Launcher</h3>
            <p class="card-desc">Quick access to HeidiSQL and any installed web-based database admin tools.</p>
          </div>
        </div>

        <div class="sub-section">
          <h4 class="sub-title">Desktop</h4>
          <div class="sub-body">
            <button class="btn-secondary" onclick={openHeidiSQL}>
              <KeyRound size={12} />
              <span>Open HeidiSQL</span>
            </button>
          </div>
        </div>

        <div class="sub-section">
          <h4 class="sub-title">Web-based</h4>
          <div class="sub-body">
            {#if webApps.length}
              <div class="action-row">
                {#each webApps as app (app.id)}
                  <button class="btn-secondary" onclick={() => openUrl(app.url)}>
                    <ExternalLink size={12} />
                    <span>{app.label}</span>
                  </button>
                {/each}
              </div>
            {:else}
              <div class="hint">No web-based DB admin tool installed, or Apache is not running. Install phpMyAdmin/Adminer under <code>apps/</code> and enable Apache in Environment &rarr; Modules.</div>
            {/if}
          </div>
        </div>
      </section>

    {:else if activeCategory === "logs"}
      <section class="tool-card">
        <div class="card-header-row">
          <FileText size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">Log Viewer</h3>
            <p class="card-desc">Last 80 lines from each engine's log, refreshed on demand.</p>
          </div>
          <button class="btn-secondary btn-xs" onclick={refreshLogs}>
            <RefreshCw size={12} />
            <span>Refresh</span>
          </button>
        </div>

        {#each LOG_SERVICE_IDS as id (id)}
          <DetailsOutput title={id} value={logs[id] || "(no log content)"} />
        {/each}
      </section>

    {:else if activeCategory === "ports"}
      <section class="tool-card">
        <div class="card-header-row">
          <Network size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">Port & Process Inspector</h3>
            <p class="card-desc">Which process is bound to each of DevPanel's known ports.</p>
          </div>
          <button class="btn-secondary btn-xs" onclick={refreshPorts}>
            <RefreshCw size={12} />
            <span>Refresh</span>
          </button>
        </div>

        <div class="server-list">
          {#each ports as p (p.port)}
            <div class="server-row">
              <div>
                <strong>{p.label}</strong>
                <span class="server-status">:{p.port} &mdash; {p.process_name ? `${p.process_name} (PID ${p.pid})` : "free"}</span>
              </div>
              {#if p.pid}
                <button class="danger-link" onclick={() => (pendingKillPid = p.pid)} disabled={toolState.busy}>Stop process</button>
              {/if}
            </div>
          {/each}
        </div>
      </section>

    {:else if activeCategory === "mail"}
      <section class="tool-card">
        <div class="card-header-row">
          <Mail size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">Mailpit</h3>
            <p class="card-desc">Web interface for locally captured outgoing email.</p>
          </div>
        </div>

        <div class="sub-section">
          <div class="sub-body">
            <button class="btn-secondary" onclick={() => openUrl(MAILPIT_URL)}>
              <Mail size={12} />
              <span>Open Mailpit Web</span>
            </button>
          </div>
        </div>
      </section>
    {/if}

    {#if output}
      <DetailsOutput value={output} />
    {/if}
  </div>
</div>

{#if pendingKillPid}
  <ConfirmDialog
    message={`Force-stop process PID ${pendingKillPid}? Any unsaved work in that process will be lost.`}
    confirmLabel="Stop process"
    onCancel={() => (pendingKillPid = null)}
    onConfirm={() => { const pid = pendingKillPid; pendingKillPid = null; killPid(pid); }}
  />
{/if}
