<script>
  // @ts-nocheck
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { invokeWith } from "#lib/tauri-utils.svelte.js";
  import Loader from "#lib/Loader.svelte";
  import CardHeader from "#lib/CardHeader.svelte";
  import DetailsOutput from "#lib/DetailsOutput.svelte";
  import { Globe, Shield, FolderKey, Sun, Moon, Laptop, Check, RefreshCw, Code2, Download } from "@lucide/svelte";

  let { theme, toggleTheme, onUpdatePreferenceChanged } = $props();

  let caTrusted = $state(false);
  let longPathsEnabled = $state(false);
  let tld = $state(".test");
  let themeMode = $state("dark"); // "system" | "dark" | "light"
  let preferredEditor = $state("vscode");
  let updateChecksEnabled = $state(true);

  let caState = $state({ busy: false, error: "", operation: "" });
  let lpState = $state({ busy: false, error: "", operation: "" });
  let savingTld = $state(false);
  /** @type {string[]} */
  let tldWarnings = $state([]);

  const TLDS = [".dp", ".dev", ".local", ".test"];

  onMount(async () => {
    caTrusted = await invoke("get_ca_trusted");
    longPathsEnabled = await invoke("get_long_paths_enabled");
    const config = await invoke("get_config");
    tld = config.tld;
    preferredEditor = config.preferred_editor || "vscode";
    updateChecksEnabled = config.update_checks_enabled !== false;

    const savedTheme = localStorage.getItem("devpanel-theme-mode") || "dark";
    themeMode = savedTheme;
    applyThemeMode(savedTheme);
  });

  function applyThemeMode(mode) {
    themeMode = mode;
    localStorage.setItem("devpanel-theme-mode", mode);
    let resolvedTheme = mode;
    if (mode === "system") {
      resolvedTheme = window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
    }
    document.documentElement.setAttribute("data-theme", resolvedTheme);
  }

  async function trustCA() {
    await invokeWith(caState, async () => {
      await invoke("trust_ca");
      caTrusted = await invoke("get_ca_trusted");
    }, "Trusting Root CA", { toastSuccess: "Root CA trusted in Windows" });
  }

  async function enableLongPaths() {
    await invokeWith(lpState, async () => {
      await invoke("enable_long_paths");
      longPathsEnabled = await invoke("get_long_paths_enabled");
    }, "Enabling Windows long paths", { toastSuccess: "NTFS long paths enabled" });
  }

  async function changeTld(newTld) {
    if (newTld === tld || savingTld) return;
    savingTld = true;
    tldWarnings = [];
    try {
      tldWarnings = await invoke("set_tld", { tld: newTld });
      tld = newTld;
    } catch (e) {
      tldWarnings = [String(e)];
    }
    savingTld = false;
  }

  async function changePreferredEditor(editor) {
    if (editor === preferredEditor) return;
    const previous = preferredEditor;
    preferredEditor = editor;
    try {
      await invoke("set_preferred_editor", { editor });
    } catch (e) {
      preferredEditor = previous;
      tldWarnings = [String(e)];
    }
  }

  async function toggleUpdateChecks() {
    const previous = updateChecksEnabled;
    updateChecksEnabled = !updateChecksEnabled;
    try {
      await invoke("set_update_checks_enabled", { enabled: updateChecksEnabled });
      onUpdatePreferenceChanged?.(updateChecksEnabled);
    } catch (e) {
      updateChecksEnabled = previous;
      tldWarnings = [String(e)];
    }
  }

</script>

<div class="settings">
  <CardHeader title="Global Settings" subtitle="Configure system preferences, appearance, local domain suffixes, and SSL." />

  <div class="grid">
    <div class="card">
      <div class="card-header">
        <Download class="icon text-accent" size={18} />
        <div>
          <h3>GitHub Updates</h3>
          <p>Check GitHub Releases once when DevPanel opens. Updates are never downloaded automatically.</p>
        </div>
      </div>
      <label class="toggle-row">
        <input type="checkbox" checked={updateChecksEnabled} onchange={toggleUpdateChecks} />
        <span>Notify me when a new version is available</span>
      </label>
    </div>
    <div class="card">
      <div class="card-header">
        <Code2 class="icon text-accent" size={18} />
        <div>
          <h3>Preferred Editor</h3>
          <p>Used by the IDE shortcut on every site card.</p>
        </div>
      </div>
      <select class="settings-select" value={preferredEditor} onchange={(event) => changePreferredEditor(event.currentTarget.value)}>
        <option value="vscode">VS Code</option>
        <option value="cursor">Cursor</option>
        <option value="sublime">Sublime Text</option>
        <option value="claude">Claude Code</option>
        <option value="codex">Codex</option>
      </select>
    </div>
    <!-- Theme & Appearance -->
    <div class="card">
      <div class="card-header">
        <Sun class="icon text-accent" size={18} />
        <div>
          <h3>Theme & Appearance</h3>
          <p>Choose your visual theme or follow OS system settings.</p>
        </div>
      </div>
      <div class="theme-options">
        <button
          class="theme-mode-btn"
          class:active={themeMode === "system"}
          onclick={() => applyThemeMode("system")}
        >
          <Laptop size={16} />
          <span>System OS (Auto)</span>
        </button>
        <button
          class="theme-mode-btn"
          class:active={themeMode === "dark"}
          onclick={() => applyThemeMode("dark")}
        >
          <Moon size={16} />
          <span>Dark Mode</span>
        </button>
        <button
          class="theme-mode-btn"
          class:active={themeMode === "light"}
          onclick={() => applyThemeMode("light")}
        >
          <Sun size={16} />
          <span>Light Mode</span>
        </button>
      </div>
    </div>

    <!-- TLD Suffix -->
    <div class="card">
      <div class="card-header">
        <Globe class="icon text-accent" size={18} />
        <div>
          <h3>Local Domain Suffix (TLD)</h3>
          <p>The domain extension assigned to your local web sites.</p>
        </div>
      </div>
      <div class="tld-selector">
        {#each TLDS as t}
          <button
            class="tld-btn"
            class:active={tld === t}
            disabled={savingTld}
            onclick={() => changeTld(t)}
          >
            {t}
          </button>
        {/each}
      </div>
      {#if tldWarnings.length > 0}
        <DetailsOutput title="TLD Update Warnings" output={tldWarnings.join("\n")} type="warning" />
      {/if}
    </div>

    <!-- SSL Certificate Trust -->
    <div class="card">
      <div class="card-header">
        <Shield class="icon text-accent" size={18} />
        <div>
          <h3>HTTPS / SSL Root Certificate</h3>
          <p>Trust DevPanel's local Certificate Authority for instant green-lock HTTPS.</p>
        </div>
      </div>
      <div class="action-row">
        <div class="status-indicator">
          {#if caTrusted}
            <span class="pill-badge success">Trusted in Windows</span>
          {:else}
            <span class="pill-badge warning">Not trusted yet</span>
          {/if}
        </div>
        <button class="btn-primary" disabled={caTrusted || caState.busy} onclick={trustCA}>
          {#if caState.busy}
            <Loader size={14} />
          {:else}
            <Check size={14} />
          {/if}
          <span>{caTrusted ? "CA Certificate Installed" : "Trust Root CA"}</span>
        </button>
      </div>
      {#if caState.error}
        <DetailsOutput title="CA Error" output={caState.error} type="error" />
      {/if}
    </div>

    <!-- Windows Long Paths -->
    <div class="card">
      <div class="card-header">
        <FolderKey class="icon text-accent" size={18} />
        <div>
          <h3>Windows NTFS Long Paths</h3>
          <p>Enable support for paths longer than 260 characters to prevent extraction errors.</p>
        </div>
      </div>
      <div class="action-row">
        <div class="status-indicator">
          {#if longPathsEnabled}
            <span class="pill-badge success">Enabled</span>
          {:else}
            <span class="pill-badge warning">Disabled</span>
          {/if}
        </div>
        <button class="btn-primary" disabled={longPathsEnabled || lpState.busy} onclick={enableLongPaths}>
          {#if lpState.busy}
            <Loader size={14} />
          {:else}
            <Check size={14} />
          {/if}
          <span>{longPathsEnabled ? "Long Paths Enabled" : "Enable Long Paths"}</span>
        </button>
      </div>
      {#if lpState.error}
        <DetailsOutput title="Long Paths Error" output={lpState.error} type="error" />
      {/if}
    </div>
  </div>
</div>

