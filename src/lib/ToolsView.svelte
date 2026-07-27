<script>
  // @ts-nocheck
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import ConfirmDialog from "#lib/ConfirmDialog.svelte";
  import { invokeWith } from "#lib/tauri-utils.svelte.js";
  import Loader from "#lib/Loader.svelte";
  import { notify } from "#lib/notifications.svelte.js";
  import DetailsOutput from "#lib/DetailsOutput.svelte";
  import WordPressSitePanel from "#lib/WordPressSitePanel.svelte";
  import {
    Database, Server, Code2, Globe, Zap, Mail,
    KeyRound, UserPlus, HardDriveDownload, Wrench, Shield, RefreshCw
  } from "@lucide/svelte";

  const categories = [
    { id: "database", label: "Database", icon: Database },
    { id: "webserver", label: "Web Server", icon: Server },
    { id: "php", label: "PHP", icon: Code2 },
    { id: "wordpress", label: "WordPress", icon: Globe },
    { id: "cache", label: "Cache", icon: Zap },
    { id: "email", label: "Email & Notifications", icon: Mail },
  ];

  let activeCategory = $state("database");
  let status = $state(null);
  let currentRootPassword = $state("");
  let newRootPassword = $state("");
  let databaseName = $state("");
  let username = $state("");
  let userPassword = $state("");
  let backups = $state([]);
  let output = $state("");
  let pending = $state(null);
  let serverTools = $state([]);
  let showRecoveryInDashboard = $state(false);
  let mailpitUrl = $state("");
  let wordpressSites = $state([]);
  let selectedWordPressSiteId = $state("");

  let toolState = $state({ busy: false, error: "", operation: "" });

  onMount(refresh);

  async function refresh() {
    try {
      await invoke("refresh_runtime_detection");
      status = await invoke("get_database_tool_status");
      showRecoveryInDashboard = (await invoke("get_config")).show_recovery_in_dashboard ?? false;
      backups = await invoke("list_database_backups");
      const [services, statuses, workspaces] = await Promise.all([
        invoke("get_services"),
        invoke("get_service_statuses"),
        invoke("list_workspaces"),
      ]);
      const stateById = new Map(statuses);
      serverTools = services.filter((service) => service.id === "apache" || service.id === "nginx").map((service) => ({ ...service, status: stateById.get(service.id) ?? "Stopped" }));
      const config = await invoke("get_config");
      mailpitUrl = config.mailpit_url || "http://localhost:8025";
      wordpressSites = workspaces.filter((workspace) => workspace.preset?.toLowerCase() === "wordpress");
      if (!wordpressSites.some((workspace) => workspace.id === selectedWordPressSiteId)) {
        selectedWordPressSiteId = wordpressSites[0]?.id ?? "";
      }
    } catch (error) {
      output = String(error);
    }
  }

  async function run(action) {
    output = "";
    toolState.error = "";
    await invokeWith(toolState, async () => {
      let msg = "Operation completed";
      if (action === "root") {
        await invoke("update_mysql_root_password", { currentPassword: currentRootPassword, newPassword: newRootPassword });
        currentRootPassword = "";
        newRootPassword = "";
        msg = "MySQL root password updated";
      } else if (action === "user") {
        if (!databaseName || !username || !userPassword) throw new Error("Fill in all 3 fields.");
        await invoke("create_database_and_user", { dbName: databaseName, username, password: userPassword });
        databaseName = "";
        username = "";
        userPassword = "";
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
      output = msg;
      notify(msg, "success");
      await refresh();
    }, "Running tool");
  }

  async function changeVersion(version) {
    output = "";
    await invokeWith(toolState, async () => {
      await invoke("set_mysql_version", { version: version || null });
      notify("MySQL version updated", "success");
      await refresh();
    }, "Changing MySQL version");
  }

  async function toggleDashboardRecovery() {
    await invoke("set_show_recovery_in_dashboard", { show: showRecoveryInDashboard });
    notify(`Recovery mode ${showRecoveryInDashboard ? "enabled" : "disabled"}`, "info");
  }

  async function serverAction(id, action) {
    output = "";
    await invokeWith(toolState, async () => {
      await invoke(`${action}_service`, { id });
      await refresh();
    }, `${action} ${id}`);
  }

  async function openMailpit() {
    try {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(mailpitUrl);
    } catch (e) {
      output = String(e);
    }
  }
</script>

<div class="tools-view">
  <div class="tools-sidebar">
    {#each categories as cat (cat.id)}
      <button
        class="cat-btn"
        class:active={activeCategory === cat.id}
        onclick={() => (activeCategory = cat.id)}
      >
        <cat.icon size={14} />
        <span>{cat.label}</span>
      </button>
    {/each}
  </div>

  <div class="tools-content">
    {#if activeCategory === "database"}
      <section class="tool-card">
        <div class="card-header-row">
          <Database size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">Database Tools (MySQL)</h3>
            <p class="card-desc">Direct management of versions, users, backups, and table repairs.</p>
          </div>
          {#if status}
            <span class="status-tag" class:running={status.mysql_running}>
              {status.mysql_running ? "Running" : status.mysql_installed ? "Stopped" : "Not installed"}
            </span>
          {/if}
        </div>

        {#if status}
          <div class="meta-info">Data path: <code>{status.data_path}</code></div>
          <div class="sub-section">
            <h4 class="sub-title">MySQL Version</h4>
            <div class="sub-body">
              <select value={status.selected_version ?? ""} onchange={(event) => changeVersion(event.currentTarget.value)} disabled={toolState.busy || status.mysql_running}>
                <option value="">Most recent detected</option>
                {#each status.mysql_versions as version (version)}<option value={version}>{version}</option>{/each}
              </select>
              {#if status.mysql_running}<div class="hint warn">Stop MySQL before switching versions.</div>{/if}
            </div>
          </div>
        {/if}

        <div class="sub-section">
          <h4 class="sub-title">Root Password</h4>
          <div class="sub-body">
            <div class="row-2col">
              <input type="password" bind:value={currentRootPassword} autocomplete="current-password" placeholder="Current password" />
              <input type="password" bind:value={newRootPassword} autocomplete="new-password" placeholder="New password" />
            </div>
            <button class="btn-secondary" onclick={() => (pending = "root")} disabled={toolState.busy}>
              <KeyRound size={12} />
              <span>Update root password</span>
            </button>
          </div>
        </div>

        <div class="sub-section">
          <h4 class="sub-title">Create Database + User</h4>
          <div class="sub-body">
            <div class="row-3col">
              <input bind:value={databaseName} placeholder="DB Name" />
              <input bind:value={username} placeholder="User" />
              <input type="password" bind:value={userPassword} autocomplete="new-password" placeholder="Password" />
            </div>
            <button class="btn-secondary" onclick={() => run("user")} disabled={toolState.busy}>
              <UserPlus size={12} />
              <span>Create Database</span>
            </button>
          </div>
        </div>

        <div class="sub-section">
          <h4 class="sub-title">Backups & Repair</h4>
          <div class="sub-body">
            <div class="action-row">
              <button class="btn-secondary" onclick={() => run("backup")} disabled={toolState.busy}>
                <HardDriveDownload size={12} />
                <span>Backup all</span>
              </button>
              <button class="btn-secondary" onclick={() => (pending = "repair")} disabled={toolState.busy}>
                <Wrench size={12} />
                <span>Repair tables</span>
              </button>
            </div>
            {#if backups.length}
              <div class="backup-list">
                {#each backups as backup (backup)}
                  <div class="backup-item">
                    <code>{backup}</code>
                    <button class="danger-link" onclick={() => (pending = `restore:${backup}`)} disabled={toolState.busy}>Restore</button>
                  </div>
                {/each}
              </div>
            {:else}
              <div class="hint">No backups generated yet.</div>
            {/if}
          </div>
        </div>
      </section>

    {:else if activeCategory === "webserver"}
      <section class="tool-card">
        <div class="card-header-row">
          <Server size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">Web Server Control</h3>
            <p class="card-desc">Start, stop, or restart Apache and Nginx services.</p>
          </div>
        </div>

        <label class="toggle-row">
          <input type="checkbox" bind:checked={showRecoveryInDashboard} onchange={toggleDashboardRecovery} />
          <span>Show recovery controls in Dashboard</span>
        </label>

        {#if serverTools.length}
          <div class="server-list">
            {#each serverTools as server (server.id)}
              <div class="server-row">
                <div>
                  <strong>{server.name}</strong>
                  <span class="server-status">{server.status}</span>
                </div>
                <div class="action-row">
                  {#each ["start", "restart", "stop"] as act (act)}
                    <button class="btn-secondary btn-xs" onclick={() => serverAction(server.id, act)} disabled={toolState.busy}>
                      {act === "start" ? "Start" : act === "restart" ? "Restart" : "Stop"}
                    </button>
                  {/each}
                </div>
              </div>
            {/each}
          </div>
        {:else}
          <div class="hint">No Apache or Nginx server detected.</div>
        {/if}
      </section>

    {:else if activeCategory === "php"}
      <section class="tool-card">
        <div class="card-header-row">
          <Code2 size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">PHP Tools</h3>
            <p class="card-desc">PHP runtime configuration belongs to the shared Environment module.</p>
          </div>
        </div>
        <div class="placeholder-content">
          <p>Install and select PHP versions in <strong>Environment</strong>. Every site uses that shared runtime; WordPress tools do not change PHP versions.</p>
        </div>
      </section>

    {:else if activeCategory === "wordpress"}
      <section class="tool-card">
        <div class="card-header-row">
          <Globe size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">WordPress Tools</h3>
            <p class="card-desc">WP-CLI and WordPress-specific utilities.</p>
          </div>
        </div>
        {#if wordpressSites.length}
          <label class="workspace-picker">
            <span>Target site</span>
            <select bind:value={selectedWordPressSiteId} disabled={toolState.busy}>
              {#each wordpressSites as site (site.id)}
                <option value={site.id}>{site.name} · {site.domain}</option>
              {/each}
            </select>
          </label>

          {#key selectedWordPressSiteId}
            <WordPressSitePanel
              workspace={wordpressSites.find((site) => site.id === selectedWordPressSiteId)}
              onUpdated={refresh}
            />
          {/key}
        {:else}
          <div class="placeholder-content">
            <p>Create a WordPress site first. Its WP-CLI tools will then be available here.</p>
          </div>
        {/if}
      </section>

    {:else if activeCategory === "cache"}
      <section class="tool-card">
        <div class="card-header-row">
          <Zap size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">Cache Tools</h3>
            <p class="card-desc">Redis and Memcached management.</p>
          </div>
        </div>
        <div class="placeholder-content">
          <p>Cache service controls — Redis CLI, cache flush, key inspection — will appear here in a future update.</p>
        </div>
      </section>

    {:else if activeCategory === "email"}
      <section class="tool-card">
        <div class="card-header-row">
          <Mail size={18} class="text-accent" />
          <div class="header-info">
            <h3 class="card-title">Email & Notifications</h3>
            <p class="card-desc">Mailpit web interface and email testing tools.</p>
          </div>
        </div>

        <div class="sub-section">
          <h4 class="sub-title">Mailpit</h4>
          <div class="sub-body">
            <p class="hint">Mailpit captures all outgoing emails from your local sites for testing.</p>
            <button class="btn-secondary" onclick={openMailpit}>
              <Mail size={12} />
              <span>Open Mailpit Web</span>
            </button>
          </div>
        </div>
      </section>
    {/if}

    {#if toolState.error || output}
      <DetailsOutput value={toolState.error || output} />
    {/if}
  </div>
</div>

{#if pending}
  <ConfirmDialog
    message={pending === "root" ? "Change MySQL root password?" : pending === "repair" ? "Repair all MySQL tables?" : "Restore backup " + pending.slice(8) + "?"}
    confirmLabel={pending.startsWith("restore:") ? "Restore" : pending === "repair" ? "Repair" : "Update"}
    onCancel={() => (pending = null)}
    onConfirm={() => { const a = pending; pending = null; run(a); }}
  />
{/if}
