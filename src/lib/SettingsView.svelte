<script>
  // @ts-nocheck
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { invokeWith } from "#lib/tauri-utils.svelte.js";
  import Loader from "#lib/Loader.svelte";
  import CardHeader from "#lib/CardHeader.svelte";
  import DetailsOutput from "#lib/DetailsOutput.svelte";
  import { FolderKey, Sun, Moon, Laptop, Check, Code2, Download } from "@lucide/svelte";

  let { theme, toggleTheme, onUpdatePreferenceChanged } = $props();

  let longPathsEnabled = $state(false);
  let themeMode = $state("dark"); // "system" | "dark" | "light"
  let preferredEditor = $state("vscode");
  let updateChecksEnabled = $state(true);

  let lpState = $state({ busy: false, error: "", operation: "" });
  /** @type {string[]} */
  let tldWarnings = $state([]);

  onMount(async () => {
    longPathsEnabled = await invoke("get_long_paths_enabled");
    const config = await invoke("get_config");
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

  async function enableLongPaths() {
    await invokeWith(lpState, async () => {
      await invoke("enable_long_paths");
      longPathsEnabled = await invoke("get_long_paths_enabled");
    }, "Enabling Windows long paths", { toastSuccess: "NTFS long paths enabled" });
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
  <CardHeader title="Global Settings" subtitle="Configure system preferences, appearance, editor, and update checks." />

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

    {#if tldWarnings.length > 0}
      <DetailsOutput title="Settings Warnings" output={tldWarnings.join("\n")} type="warning" />
    {/if}

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

