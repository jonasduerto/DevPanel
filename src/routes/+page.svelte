<script>
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { attachConsole } from "@tauri-apps/plugin-log";
  import PanelView from "#lib/PanelView.svelte";
  import WorkspacesView from "#lib/WorkspacesView.svelte";
  import SettingsView from "#lib/SettingsView.svelte";
  import ToolsView from "#lib/ToolsView.svelte";
  import ModulesView from "#lib/ModulesView.svelte";
  import Toast from "#lib/Toast.svelte";
  import LoadingBar from "#lib/LoadingBar.svelte";
  import StatusBar from "#lib/StatusBar.svelte";
  import { subscribeToasts, subscribeLoading, subscribeStatus } from "#lib/notifications.svelte.js";
  import { LayoutDashboard, Globe, Settings, Wrench, Sun, Moon, X, Cpu } from "@lucide/svelte";
  import { locale, t, loadDictionary } from "#lib/i18n/index.js";

  let view = $state("panel");
  let theme = $state("dark");
  const appWindow = getCurrentWindow();

  /** @type {Array<{id: number, message: string, type: string}>} */
  let toasts = $state([]);
  let loading = $state({ busy: false, operation: "" });
  let statusMessage = $state("");

  onMount(() => {
    const userLocale = localStorage.getItem("devpanel-locale") || "en";
    loadDictionary(userLocale);
    locale.set(userLocale);

    // Theme initialization
    const themeMode = localStorage.getItem("devpanel-theme-mode") || "dark";
    let activeTheme = themeMode;
    if (themeMode === "system") {
      activeTheme = window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
    } else if (themeMode !== "light" && themeMode !== "dark") {
      activeTheme = "dark";
    }
    theme = activeTheme;
    document.documentElement.setAttribute("data-theme", theme);

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    /** @param {MediaQueryListEvent} e */
    const handleThemeChange = (e) => {
      if (localStorage.getItem("devpanel-theme-mode") === "system") {
        theme = e.matches ? "dark" : "light";
        document.documentElement.setAttribute("data-theme", theme);
      }
    };
    mediaQuery.addEventListener("change", handleThemeChange);

    /** @type {(() => void) | undefined} */
    let detachConsole;
    attachConsole().then((detach) => {
      detachConsole = detach;
    });

    const unsubToasts = subscribeToasts((event) => {
      if ("dismiss" in event) {
        toasts = toasts.filter((/** @type {any} */ t) => t.id !== event.id);
      } else {
        toasts = [...toasts, event.toast];
      }
    });
    const unsubLoading = subscribeLoading((event) => {
      loading = event;
    });
    const unsubStatus = subscribeStatus((msg) => {
      statusMessage = msg;
    });
    return () => {
      unsubToasts();
      unsubLoading();
      unsubStatus();
      mediaQuery.removeEventListener("change", handleThemeChange);
      if (detachConsole) detachConsole();
    };
  });

  function toggleTheme() {
    theme = theme === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", theme);
    localStorage.setItem("devpanel-theme-mode", theme);
  }

  function closePanel() {
    appWindow.hide();
  }

  appWindow.onCloseRequested((event) => {
    event.preventDefault();
    appWindow.hide();
  });
</script>

<LoadingBar busy={loading.busy} operation={loading.operation} />
<Toast {toasts} />

<div class="panel-container" data-theme={theme}>
  <header class="titlebar" data-tauri-drag-region>
    <div class="brand">
      <div class="logo-box"><img src="/branding/devpanel-mark.svg" alt="DevPanel" /></div>
      <div class="brand-text">
        <span class="title">DevPanel</span>
        <span class="version-badge">BETA</span>
      </div>
    </div>

    <nav class="nav-tabs" data-tauri-drag-region="false">
      <button
        class="tab-btn"
        class:active={view === "panel"}
        onclick={() => (view = "panel")}
        title="Dashboard Overview"
      >
        <LayoutDashboard size={14} />
        <span>Dashboard</span>
      </button>
      <button
        class="tab-btn"
        class:active={view === "workspaces"}
        onclick={() => (view = "workspaces")}
        title="Sites"
      >
        <Globe size={14} />
        <span>Sites</span>
      </button>
      <button
        class="tab-btn"
        class:active={view === "modules" || view === "addons"}
        onclick={() => (view = "modules")}
        title="Modules"
      >
        <Cpu size={14} />
        <span>Modules</span>
      </button>
      <button
        class="tab-btn"
        class:active={view === "settings"}
        onclick={() => (view = "settings")}
        title="Settings"
      >
        <Settings size={14} />
        <span>Settings</span>
      </button>
      <button
        class="tab-btn"
        class:active={view === "tools"}
        onclick={() => (view = "tools")}
        title="Tools"
      >
        <Wrench size={14} />
        <span>Tools</span>
      </button>
    </nav>

    <div class="window-actions" data-tauri-drag-region="false">
      <button class="icon-btn theme-btn" onclick={toggleTheme} title={theme === "dark" ? "Switch to Light Mode" : "Switch to Dark Mode"}>
        {#if theme === "dark"}
          <Sun size={15} />
        {:else}
          <Moon size={15} />
        {/if}
      </button>
      <div class="divider"></div>
      <button class="icon-btn close-btn" onclick={closePanel} title="Close window">
        <X size={15} />
      </button>
    </div>
  </header>

  <main class="content">
    {#if view === "panel"}
      <PanelView onNavigateToSites={() => (view = "workspaces")} />
    {:else if view === "workspaces"}
      <WorkspacesView />
    {:else if view === "modules" || view === "addons"}
      <ModulesView />
    {:else if view === "settings"}
      <SettingsView {theme} {toggleTheme} />
    {:else}
      <ToolsView />
    {/if}
  </main>

  <StatusBar message={statusMessage} />
</div>

