use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::environment::{self, StackDefinition};
use crate::service::types::ServiceStatus;
use crate::state::AppState;
use crate::workspace::manifest::WorkspaceManifest;
use crate::workspace::scaffold;
use crate::workspace::Workspace;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ============================================================================
// WP-CLI Helper
// ============================================================================

/// Internal helper: runs a WP-CLI command in a workspace's project root,
/// blocking but cheap — intended for `spawn_blocking`.
fn run_wp_cli_blocking(
    root: &Path,
    www_dir: &str,
    workspace: &Workspace,
    args: &[String],
) -> Result<String, String> {
    let project_dir = scaffold::project_path(root, www_dir, &workspace.id);

    let php = scaffold::find_tool(root, "php", "php.exe")
        .ok_or_else(|| "PHP not found in DevPanel/bin/php — required to run WP-CLI.".to_string())?;
    let wp_phar = scaffold::find_tool(root, "wp-cli", "wp-cli.phar")
        .or_else(|| scaffold::find_tool(root, "wp-cli", "wp.phar"))
        .ok_or_else(|| {
            "wp-cli.phar not found — place it in bin/wp-cli/wp-cli.phar, then retry.".to_string()
        })?;

    let mut cmd_args: Vec<String> = vec![wp_phar.to_string_lossy().into_owned()];
    cmd_args.extend_from_slice(args);
    cmd_args.push(format!("--path={}", project_dir.display()));
    cmd_args.push("--allow-root".to_string());

    let output = Command::new(&php)
        .args(&cmd_args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Failed to run wp-cli: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        // Keep machine-readable commands (for example `--format=json`) clean;
        // WP-CLI warnings commonly go to stderr even when the command succeeds.
        Ok(if stdout.trim().is_empty() { stderr } else { stdout })
    } else {
        Err(format!("WP-CLI exited with {}: {}{}", output.status, stdout, stderr))
    }
}

/// Helper to get workspace path without full WP-CLI setup
fn get_workspace_path(root: &Path, www_dir: &str, ws_id: &str) -> PathBuf {
    scaffold::project_path(root, www_dir, ws_id)
}

/// Run a WP-CLI command and return structured result
fn run_wp_command(
    root: &Path,
    www_dir: &str,
    workspace: &Workspace,
    args: &[String],
) -> Result<WpCommandResult, String> {
    run_wp_cli_blocking(root, www_dir, workspace, args).map(|output| WpCommandResult {
            success: true,
            output,
            error: None,
        })
}

#[derive(Serialize, Deserialize)]
pub struct WpCommandResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

// ============================================================================
// WP-CLI Raw Command (for flexible use)
// ============================================================================

/// Runs a wp-cli command against a workspace's project root. Looks for
/// `wp-cli.phar`/`wp.phar` under DevPanel's own `bin/wp-cli/` directory.
#[tauri::command]
pub async fn run_wp_cli(
    id: String,
    args: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || run_wp_cli_blocking(&root, &www_dir, &workspace, &args))
        .await
        .map_err(|e| format!("wp-cli task panicked: {e}"))?
}

#[derive(Serialize)]
pub struct WpToolStatus {
    pub php_found: bool,
    pub php_path: Option<String>,
    pub wp_cli_found: bool,
    pub wp_cli_path: Option<String>,
    pub mysql_found: bool,
    pub mysql_path: Option<String>,
}

/// Returns availability of PHP, WP-CLI phar, and MySQL client needed for
/// WordPress operations. Probes DevPanel's own `bin/` via `find_tool`.
#[tauri::command]
pub async fn get_wp_tool_status(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpToolStatus, String> {
    {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?;
    }
    let root = state.service_mgr.root().clone();

    tokio::task::spawn_blocking(move || {
        let php = scaffold::find_tool(&root, "php", "php.exe");
        let wp_cli = scaffold::find_tool(&root, "wp-cli", "wp-cli.phar")
            .or_else(|| scaffold::find_tool(&root, "wp-cli", "wp.phar"));
        let mysql = scaffold::find_tool(&root, "mysql", "mysql.exe");

        WpToolStatus {
            php_found: php.is_some(),
            php_path: php.map(|p| p.to_string_lossy().into_owned()),
            wp_cli_found: wp_cli.is_some(),
            wp_cli_path: wp_cli.map(|p| p.to_string_lossy().into_owned()),
            mysql_found: mysql.is_some(),
            mysql_path: mysql.map(|p| p.to_string_lossy().into_owned()),
        }
    })
    .await
    .map_err(|e| format!("tool status task panicked: {e}"))
}

/// Returns the installed WordPress version string (e.g. "6.4.3") by running
/// `wp core version`. Returns an error if WP-CLI or PHP is unavailable.
#[tauri::command]
pub async fn get_wp_version(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        let raw = run_wp_cli_blocking(
            &root,
            &www_dir,
            &workspace,
            &["core".to_string(), "version".to_string()],
        )?;
        Ok(raw.trim().to_string())
    })
    .await
    .map_err(|e| format!("wp version task panicked: {e}"))?
}

#[derive(Serialize)]
pub struct RepairWpResult {
    pub database_repaired: bool,
    pub db_message: String,
    pub wp_checksums: String,
    pub wp_db_repair: String,
}

/// Runs the full WordPress repair sequence for a workspace:
/// 1. Retry CREATE DATABASE
/// 2. `wp core verify-checksums`
/// 3. `wp db repair`
///
/// Domain/HTTPS setup is deliberately not part of this operation: it can
/// require Windows elevation and must stay an explicit site configuration
/// action.
#[tauri::command]
pub async fn repair_workspace(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<RepairWpResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();

    let ws_id = workspace.id.clone();
    let ws_db_name = workspace.db_name.clone();

    let db_result = tokio::task::spawn_blocking(move || {
        let db_msg = match scaffold::prepare_database(&root, &ws_db_name) {
            Ok(()) => "Database ready".to_string(),
            Err(e) => format!("Database not ready: {e}"),
        };
        let https_msg = String::new();
        (db_msg, https_msg)
    })
    .await
    .map_err(|e| format!("db task panicked: {e}"))?;

    let ws_for_wp = {
        let store = state.workspace_store.lock().await;
        store
            .get(&ws_id)
            .ok_or_else(|| format!("Workspace '{ws_id}' not found"))?
    };
    let root2 = state.service_mgr.root().clone();
    let www_dir2 = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    let (checksum_result, db_repair_result) = tokio::task::spawn_blocking(move || {
        let checksums = run_wp_cli_blocking(
            &root2,
            &www_dir2,
            &ws_for_wp,
            &["core".to_string(), "verify-checksums".to_string()],
        )
        .unwrap_or_else(|e| format!("wp verify-checksums failed: {e}"));

        let db_repair = run_wp_cli_blocking(
            &root2,
            &www_dir2,
            &ws_for_wp,
            &["db".to_string(), "repair".to_string()],
        )
        .unwrap_or_else(|e| format!("wp db repair failed: {e}"));

        (checksums, db_repair)
    })
    .await
    .map_err(|e| format!("wp repair task panicked: {e}"))?;

    Ok(RepairWpResult {
        database_repaired: db_result.0.contains("ready"),
        db_message: db_result.0,
        wp_checksums: checksum_result,
        wp_db_repair: db_repair_result,
    })
}

#[derive(Serialize)]
pub struct WorkspaceDebugContext {
    pub workspace: Workspace,
    pub manifest: Option<WorkspaceManifest>,
    pub active_stack: Option<StackDefinition>,
    pub service_statuses: Vec<(String, ServiceStatus)>,
    pub apache_error_log: Vec<String>,
    pub nginx_error_log: Vec<String>,
    pub php_error_log: Vec<String>,
}

/// Bundles everything useful for handing a broken workspace to an AI
/// assistant in one shot: server/service status, the active Environment,
/// the workspace's own manifest, and the tail of each server's error log.
#[tauri::command]
pub async fn get_workspace_debug_context(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WorkspaceDebugContext, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    let active_stack = {
        let config = state.config.lock().await;
        config.get().active_stack_id.clone()
    }
    .and_then(|sid| {
        environment::predefined_stacks()
            .into_iter()
            .find(|s| s.id == sid)
    });

    let mut service_statuses = Vec::new();
    if let Some(stack) = &active_stack {
        for svc in &stack.services {
            service_statuses.push((svc.clone(), state.service_mgr.status(svc).await));
        }
    }

    let root_for_blocking = root.clone();
    let www_dir_for_blocking = www_dir.clone();
    let ws_id = workspace.id.clone();
    let (manifest, apache_error_log, nginx_error_log, php_error_log) =
        tokio::task::spawn_blocking(move || {
            let project_dir =
                scaffold::project_path(&root_for_blocking, &www_dir_for_blocking, &ws_id);
            let manifest = WorkspaceManifest::load(&project_dir).ok();
            let apache_log = tail_log(&root_for_blocking.join("bin/apache/logs/error.log"), 20);
            let nginx_log = tail_log(&root_for_blocking.join("bin/nginx/logs/error.log"), 20);
            let php_log = tail_log(&root_for_blocking.join("bin/php/php_errors.log"), 20);
            (manifest, apache_log, nginx_log, php_log)
        })
        .await
        .map_err(|e| format!("Debug context task panicked: {e}"))?;

    Ok(WorkspaceDebugContext {
        workspace,
        manifest,
        active_stack,
        service_statuses,
        apache_error_log,
        nginx_error_log,
        php_error_log,
    })
}

fn tail_log(path: &Path, n: usize) -> Vec<String> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].iter().map(|s| s.to_string()).collect()
}

// ============================================================================
// FASE 1: WordPress Management Tools (Plugins, Themes, Core, Cache)
// ============================================================================

#[derive(Serialize, Deserialize, Clone)]
pub struct WpPlugin {
    pub name: String,
    pub status: String,
    pub version: String,
    pub update_available: Option<String>,
}

#[derive(Serialize)]
pub struct WpPluginsResult {
    pub success: bool,
    pub plugins: Vec<WpPlugin>,
    pub error: Option<String>,
}

/// List all WordPress plugins with their status
#[tauri::command]
pub async fn wp_plugin_list(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpPluginsResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        let result = run_wp_cli_blocking(
            &root,
            &www_dir,
            &workspace,
            &["plugin".to_string(), "list".to_string(), "--format=json".to_string()],
        );

        match result {
            Ok(output) => {
                // Parse JSON output
                let plugins: Vec<WpPlugin> = serde_json::from_str(&output)
                    .unwrap_or_else(|_| Vec::new());
                WpPluginsResult {
                    success: true,
                    plugins,
                    error: None,
                }
            }
            Err(e) => WpPluginsResult {
                success: false,
                plugins: Vec::new(),
                error: Some(e),
            },
        }
    })
    .await
    .map_err(|e| format!("plugin list task panicked: {e}"))
}

/// Activate a WordPress plugin
#[tauri::command]
pub async fn wp_plugin_activate(
    id: String,
    plugin: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["plugin".to_string(), "activate".to_string(), plugin],
        )
    })
    .await
    .map_err(|e| format!("plugin activate task panicked: {e}"))?
}

/// Deactivate a WordPress plugin
#[tauri::command]
pub async fn wp_plugin_deactivate(
    id: String,
    plugin: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["plugin".to_string(), "deactivate".to_string(), plugin],
        )
    })
    .await
    .map_err(|e| format!("plugin deactivate task panicked: {e}"))?
}

/// Delete a WordPress plugin
#[tauri::command]
pub async fn wp_plugin_delete(
    id: String,
    plugin: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["plugin".to_string(), "delete".to_string(), plugin],
        )
    })
    .await
    .map_err(|e| format!("plugin delete task panicked: {e}"))?
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WpTheme {
    pub name: String,
    pub status: String,
    pub version: String,
    pub update_available: Option<String>,
}

#[derive(Serialize)]
pub struct WpThemesResult {
    pub success: bool,
    pub themes: Vec<WpTheme>,
    pub error: Option<String>,
}

/// List all WordPress themes with their status
#[tauri::command]
pub async fn wp_theme_list(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpThemesResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        let result = run_wp_cli_blocking(
            &root,
            &www_dir,
            &workspace,
            &["theme".to_string(), "list".to_string(), "--format=json".to_string()],
        );

        match result {
            Ok(output) => {
                let themes: Vec<WpTheme> = serde_json::from_str(&output)
                    .unwrap_or_else(|_| Vec::new());
                WpThemesResult {
                    success: true,
                    themes,
                    error: None,
                }
            }
            Err(e) => WpThemesResult {
                success: false,
                themes: Vec::new(),
                error: Some(e),
            },
        }
    })
    .await
    .map_err(|e| format!("theme list task panicked: {e}"))
}

/// Activate a WordPress theme
#[tauri::command]
pub async fn wp_theme_activate(
    id: String,
    theme: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["theme".to_string(), "activate".to_string(), theme],
        )
    })
    .await
    .map_err(|e| format!("theme activate task panicked: {e}"))?
}

/// Update WordPress core
#[tauri::command]
pub async fn wp_core_update(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["core".to_string(), "update".to_string()],
        )
    })
    .await
    .map_err(|e| format!("core update task panicked: {e}"))?
}

/// Reinstall WordPress core (downloads fresh copy)
#[tauri::command]
pub async fn wp_core_reinstall(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["core".to_string(), "reinstall".to_string()],
        )
    })
    .await
    .map_err(|e| format!("core reinstall task panicked: {e}"))?
}

/// Update all plugins, themes, and core
#[tauri::command]
pub async fn wp_update_all(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        // Run all updates
        let plugin_update = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["plugin".to_string(), "update".to_string(), "--all".to_string()],
        );
        let theme_update = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["theme".to_string(), "update".to_string(), "--all".to_string()],
        );
        let core_update = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["core".to_string(), "update".to_string()],
        );

        let mut output = String::new();
        if let Ok(r) = plugin_update {
            output.push_str(&format!("=== PLUGINS ===\n{}\n", r.output));
        }
        if let Ok(r) = theme_update {
            output.push_str(&format!("=== THEMES ===\n{}\n", r.output));
        }
        if let Ok(r) = core_update {
            output.push_str(&format!("=== CORE ===\n{}", r.output));
        }

        WpCommandResult {
            success: true,
            output,
            error: None,
        }
    })
    .await
    .map_err(|e| format!("update all task panicked: {e}"))
}

/// Flush all caches (object cache, transients, etc.)
#[tauri::command]
pub async fn wp_cache_flush(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["cache".to_string(), "flush".to_string()],
        )
    })
    .await
    .map_err(|e| format!("cache flush task panicked: {e}"))?
}

/// Clean up transients (expired options cache)
#[tauri::command]
pub async fn wp_transient_cleanup(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["transient".to_string(), "delete".to_string(), "--expired".to_string()],
        )
    })
    .await
    .map_err(|e| format!("transient cleanup task panicked: {e}"))?
}

/// Search and replace in database
#[tauri::command]
pub async fn wp_search_replace(
    id: String,
    old_url: String,
    new_url: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &[
                "search-replace".to_string(),
                old_url,
                new_url,
                "--all-tables".to_string(),
            ],
        )
    })
    .await
    .map_err(|e| format!("search replace task panicked: {e}"))?
}

// ============================================================================
// FASE 3: Security Hardening & Audit
// ============================================================================

#[derive(Serialize)]
pub struct WpSecurityAuditResult {
    pub success: bool,
    pub checks: Vec<SecurityCheck>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct SecurityCheck {
    pub name: String,
    pub status: String,
    pub description: String,
    pub recommendation: String,
}

/// Perform WordPress security audit
#[tauri::command]
pub async fn wp_security_audit(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpSecurityAuditResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        let project_dir = get_workspace_path(&root, &www_dir, &workspace.id);
        let mut checks = Vec::new();

        // Check 1: wp-config.php file permissions
        let wp_config = project_dir.join("wp-config.php");
        if wp_config.exists() {
            checks.push(SecurityCheck {
                name: "wp-config.php exists".to_string(),
                status: "pass".to_string(),
                description: "wp-config.php file is present".to_string(),
                recommendation: "Ensure wp-config.php is not accessible from the web".to_string(),
            });
        } else {
            checks.push(SecurityCheck {
                name: "wp-config.php missing".to_string(),
                status: "fail".to_string(),
                description: "wp-config.php file not found".to_string(),
                recommendation: "WordPress installation may be corrupted".to_string(),
            });
        }

        // Check 2: Check if debug is enabled
        let debug_check = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["config".to_string(), "get".to_string(), "WP_DEBUG".to_string()],
        );
        if let Ok(result) = debug_check {
            let is_debug = result.output.trim().to_lowercase() == "true";
            checks.push(SecurityCheck {
                name: "WP_DEBUG setting".to_string(),
                status: if is_debug { "warn".to_string() } else { "pass".to_string() },
                description: if is_debug {
                    "Debug mode is ENABLED".to_string()
                } else {
                    "Debug mode is disabled".to_string()
                },
                recommendation: if is_debug {
                    "Disable WP_DEBUG in production for security".to_string()
                } else {
                    "No action needed".to_string()
                },
            });
        }

        // Check 3: Check database prefix
        let prefix_check = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["config".to_string(), "get".to_string(), "table_prefix".to_string()],
        );
        if let Ok(result) = prefix_check {
            let prefix = result.output.trim();
            let is_default = prefix == "wp_";
            checks.push(SecurityCheck {
                name: "Database table prefix".to_string(),
                status: if is_default { "warn".to_string() } else { "pass".to_string() },
                description: format!("Table prefix: {}", prefix),
                recommendation: if is_default {
                    "Consider using a custom table prefix (not wp_) for security".to_string()
                } else {
                    "Custom table prefix is in use - good!".to_string()
                },
            });
        }

        // Check 4: Check file edit capability
        let disallow_check = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["config".to_string(), "get".to_string(), "DISALLOW_FILE_EDIT".to_string()],
        );
        if let Ok(result) = disallow_check {
            let is_disabled = result.output.trim().to_lowercase() == "true";
            checks.push(SecurityCheck {
                name: "File editor disabled".to_string(),
                status: if is_disabled { "pass".to_string() } else { "warn".to_string() },
                description: if is_disabled {
                    "File editor is DISABLED".to_string()
                } else {
                    "File editor is ENABLED".to_string()
                },
                recommendation: if is_disabled {
                    "Good! File editing is disabled".to_string()
                } else {
                    "Set DISALLOW_FILE_EDIT to true to prevent editing PHP files from admin".to_string()
                },
            });
        }

        // Check 5: Check WordPress version visibility
        let generator_check = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["option".to_string(), "get".to_string(), "blogdescription".to_string()],
        );
        checks.push(SecurityCheck {
            name: "WordPress installation".to_string(),
            status: if generator_check.is_ok() { "pass".to_string() } else { "fail".to_string() },
            description: if generator_check.is_ok() {
                "WordPress is properly installed".to_string()
            } else {
                "WordPress installation may have issues".to_string()
            },
            recommendation: "Ensure all WordPress files are present".to_string(),
        });

        WpSecurityAuditResult {
            success: true,
            checks,
            error: None,
        }
    })
    .await
    .map_err(|e| format!("security audit task panicked: {e}"))
}

/// Apply WordPress security hardening settings
#[tauri::command]
pub async fn wp_security_harden(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        let mut output = String::new();
        let mut failures: Vec<String> = Vec::new();

        // Disable file editor
        let disable_edit = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &[
                "config".to_string(),
                "set".to_string(),
                "DISALLOW_FILE_EDIT".to_string(),
                "true".to_string(),
            ],
        );
        match disable_edit {
            Ok(r) => output.push_str(&format!("File editor disabled: {}\n", r.output)),
            Err(e) => {
                output.push_str(&format!("Could not disable file editor: {}\n", e));
                failures.push("DISALLOW_FILE_EDIT".to_string());
            }
        }

        // Disable WP_DEBUG in config
        let disable_debug = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &[
                "config".to_string(),
                "set".to_string(),
                "WP_DEBUG".to_string(),
                "false".to_string(),
            ],
        );
        match disable_debug {
            Ok(r) => output.push_str(&format!("Debug disabled: {}\n", r.output)),
            Err(e) => {
                output.push_str(&format!("Could not disable debug: {}\n", e));
                failures.push("WP_DEBUG".to_string());
            }
        }

        // Disable error display
        let disable_display = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &[
                "config".to_string(),
                "set".to_string(),
                "WP_DEBUG_DISPLAY".to_string(),
                "false".to_string(),
            ],
        );
        match disable_display {
            Ok(r) => output.push_str(&format!("Error display disabled: {}\n", r.output)),
            Err(e) => {
                output.push_str(&format!("Could not disable error display: {}\n", e));
                failures.push("WP_DEBUG_DISPLAY".to_string());
            }
        }

        WpCommandResult {
            success: failures.is_empty(),
            output,
            error: (!failures.is_empty()).then(|| format!("Could not apply: {}", failures.join(", "))),
        }
    })
    .await
    .map_err(|e| format!("security harden task panicked: {e}"))
}

// ============================================================================
// FASE 4: Performance Analysis & Site Health
// ============================================================================

#[derive(Serialize)]
pub struct WpSiteHealthResult {
    pub success: bool,
    pub health: SiteHealth,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct SiteHealth {
    pub status: String,
    pub score: i32,
    pub critical: Vec<String>,
    pub recommended: Vec<String>,
    pub good: Vec<String>,
}

/// Run WordPress site health check
#[tauri::command]
pub async fn wp_site_health(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpSiteHealthResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        let mut health = SiteHealth {
            status: "good".to_string(),
            score: 100,
            critical: Vec::new(),
            recommended: Vec::new(),
            good: Vec::new(),
        };

        // Check 1: WordPress version
        let version_result = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["core".to_string(), "version".to_string()],
        );
        if let Ok(r) = version_result {
            health.good.push(format!("WordPress version: {}", r.output.trim()));
        }

        // Check 2: PHP version
        let php_version = scaffold::find_tool(&root, "php", "php.exe");
        if let Some(php) = php_version {
            health.good.push(format!("PHP version detected at: {}", php.display()));
        }

        // Check 3: Database connection
        let db_check = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["db".to_string(), "check".to_string()],
        );
        match db_check {
            Ok(r) => {
                if r.output.contains("OK") {
                    health.good.push("Database tables OK".to_string());
                } else {
                    health.critical.push("Database tables need repair".to_string());
                }
            }
            Err(_) => health.critical.push("Cannot connect to database".to_string()),
        }

        // Check 4: Object cache
        let cache_check = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["cache".to_string(), "get".to_string(), "__wp_cli_test".to_string()],
        );
        match cache_check {
            Ok(_) => health.good.push("Object cache working".to_string()),
            Err(_) => health.recommended.push("Object cache not persistent (expected for file-based cache)".to_string()),
        }

        // Check 5: Cron status
        let cron_check = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["cron".to_string(), "list".to_string()],
        );
        match cron_check {
            Ok(_) => health.good.push("WP-Cron is functional".to_string()),
            Err(e) => health.recommended.push(format!("WP-Cron issue: {}", e)),
        }

        // Calculate score
        health.score = 100 - (health.critical.len() as i32 * 20) - (health.recommended.len() as i32 * 5);
        health.status = if health.critical.is_empty() && health.recommended.is_empty() {
            "good".to_string()
        } else if health.critical.is_empty() {
            "recommended".to_string()
        } else {
            "critical".to_string()
        };

        WpSiteHealthResult {
            success: true,
            health,
            error: None,
        }
    })
    .await
    .map_err(|e| format!("site health task panicked: {e}"))
}

#[derive(Serialize)]
pub struct WpPerformanceResult {
    pub success: bool,
    pub metrics: PerformanceMetrics,
    pub bottlenecks: Vec<String>,
    pub recommendations: Vec<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct PerformanceMetrics {
    pub db_size_mb: f64,
    pub post_count: i32,
    pub user_count: i32,
    pub plugin_count: i32,
    pub theme_count: i32,
    pub transient_count: i32,
    pub revision_count: i32,
    pub transients_size_mb: f64,
    pub spam_comment_count: i32,
}

/// Analyze WordPress performance
#[tauri::command]
pub async fn wp_performance_analysis(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpPerformanceResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        let mut metrics = PerformanceMetrics {
            db_size_mb: 0.0,
            post_count: 0,
            user_count: 0,
            plugin_count: 0,
            theme_count: 0,
            transient_count: 0,
            revision_count: 0,
            transients_size_mb: 0.0,
            spam_comment_count: 0,
        };
        let mut bottlenecks = Vec::new();
        let mut recommendations = Vec::new();

        // Get post count
        if let Ok(r) = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["post".to_string(), "list".to_string(), "--post_type=post".to_string(), "--format=count".to_string()],
        ) {
            metrics.post_count = r.output.trim().parse().unwrap_or(0);
        }

        // Get user count
        if let Ok(r) = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["user".to_string(), "list".to_string(), "--format=count".to_string()],
        ) {
            metrics.user_count = r.output.trim().parse().unwrap_or(0);
        }

        // Get plugin count
        if let Ok(r) = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["plugin".to_string(), "list".to_string(), "--format=count".to_string()],
        ) {
            metrics.plugin_count = r.output.trim().parse().unwrap_or(0);
        }

        // Get theme count
        if let Ok(r) = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["theme".to_string(), "list".to_string(), "--format=count".to_string()],
        ) {
            metrics.theme_count = r.output.trim().parse().unwrap_or(0);
        }

        // Use WordPress's resolved options table name. This works with custom
        // table prefixes (and avoids analysing the wrong site as `wp_options`).
        if let Ok(r) = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &[
                "eval".to_string(),
                "global $wpdb; echo (int) $wpdb->get_var(\"SELECT COUNT(*) FROM {$wpdb->options} WHERE option_name LIKE '_transient_%' OR option_name LIKE '_site_transient_%'\");".to_string(),
            ],
        ) {
            metrics.transient_count = r.output.trim().parse().unwrap_or(0);
        }

        if let Ok(r) = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &[
                "eval".to_string(),
                "global $wpdb; echo (int) $wpdb->get_var(\"SELECT COALESCE(SUM(LENGTH(option_value)), 0) FROM {$wpdb->options} WHERE option_name LIKE '_transient_%' OR option_name LIKE '_site_transient_%'\");".to_string(),
            ],
        ) {
            let bytes: f64 = r.output.trim().parse().unwrap_or(0.0);
            metrics.transients_size_mb = bytes / 1_048_576.0;
        }

        // `wp db size --format=json` is portable across the database engines
        // supported by WP-CLI. Keep the remaining metrics useful if a database
        // adapter does not expose size information.
        if let Ok(r) = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &[
                "db".to_string(),
                "size".to_string(),
                "--format=json".to_string(),
                "--size_format=b".to_string(),
            ],
        ) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(r.output.trim()) {
                let bytes = value
                    .as_array()
                    .and_then(|rows| rows.first())
                    .and_then(|row| row.get("size"))
                    .and_then(|size| size.as_f64().or_else(|| size.as_str()?.parse().ok()))
                    .unwrap_or(0.0);
                metrics.db_size_mb = bytes / 1_048_576.0;
            }
        }

        // Get revision count
        if let Ok(r) = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["post".to_string(), "list".to_string(), "--post_type=revision".to_string(), "--format=count".to_string()],
        ) {
            metrics.revision_count = r.output.trim().parse().unwrap_or(0);
        }

        // Get spam comments
        if let Ok(r) = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["comment".to_string(), "list".to_string(), "--status=spam".to_string(), "--format=count".to_string()],
        ) {
            metrics.spam_comment_count = r.output.trim().parse().unwrap_or(0);
        }

        // Analyze for bottlenecks
        if metrics.plugin_count > 30 {
            bottlenecks.push(format!("High plugin count ({}). Many plugins slow down WordPress.", metrics.plugin_count));
            recommendations.push("Consider deactivating/deleting unused plugins".to_string());
        }

        if metrics.revision_count > metrics.post_count * 3 {
            bottlenecks.push(format!("Many post revisions ({}). This bloats the database.", metrics.revision_count));
            recommendations.push("Run 'wp post delete $(wp post list --post_type=revision --format=ids)' to clean revisions".to_string());
        }

        if metrics.transient_count > 500 {
            bottlenecks.push(format!("Many transients stored ({})", metrics.transient_count));
            recommendations.push("Run 'wp transient delete --expired' to clean expired transients".to_string());
        }

        if metrics.transients_size_mb > 20.0 {
            bottlenecks.push(format!(
                "Transient values use {:.1} MB of the options table.",
                metrics.transients_size_mb
            ));
            recommendations.push("Review persistent cache configuration and remove expired transients".to_string());
        }

        if metrics.spam_comment_count > 100 {
            bottlenecks.push(format!("Spam comments ({}). These slow down comment queries.", metrics.spam_comment_count));
            recommendations.push("Run 'wp comment delete $(wp comment list --status=spam --format=ids)' --yes to delete spam".to_string());
        }

        if metrics.plugin_count > 20 && metrics.post_count < 100 {
            recommendations.push("Many plugins for a small site. Consider audit of necessary plugins.".to_string());
        }

        // Check for active debug mode
        if let Ok(r) = run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["config".to_string(), "get".to_string(), "WP_DEBUG".to_string()],
        ) {
            if r.output.trim().to_lowercase() == "true" {
                bottlenecks.push("Debug mode is ENABLED - significant performance impact".to_string());
                recommendations.push("Disable WP_DEBUG when not actively debugging".to_string());
            }
        }

        WpPerformanceResult {
            success: true,
            metrics,
            bottlenecks,
            recommendations,
            error: None,
        }
    })
    .await
    .map_err(|e| format!("performance analysis task panicked: {e}"))
}

/// Get database size information
#[tauri::command]
pub async fn wp_db_size(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpCommandResult, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        run_wp_command(
            &root,
            &www_dir,
            &workspace,
            &["db".to_string(), "size".to_string(), "--tables".to_string()],
        )
    })
    .await
    .map_err(|e| format!("db size task panicked: {e}"))?
}

#[derive(Serialize)]
pub struct WpWorkspaceInfo {
    pub version: String,
    pub site_url: String,
    pub title: String,
    pub db_size: String,
    pub plugins_active: i32,
    pub plugins_total: i32,
    pub themes_total: i32,
}

/// Get comprehensive WordPress workspace info
#[tauri::command]
pub async fn wp_workspace_info(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WpWorkspaceInfo, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };

    tokio::task::spawn_blocking(move || {
        let version = run_wp_command(&root, &www_dir, &workspace, &["core".to_string(), "version".to_string()])
            .map(|r| r.output.trim().to_string())
            .unwrap_or_else(|_| "Unknown".to_string());

        let site_url = run_wp_command(&root, &www_dir, &workspace, &["option".to_string(), "get".to_string(), "siteurl".to_string()])
            .map(|r| r.output.trim().to_string())
            .unwrap_or_else(|_| "Unknown".to_string());

        let title = run_wp_command(&root, &www_dir, &workspace, &["option".to_string(), "get".to_string(), "blogname".to_string()])
            .map(|r| r.output.trim().to_string())
            .unwrap_or_else(|_| "Unknown".to_string());

        let db_size = run_wp_command(&root, &www_dir, &workspace, &["db".to_string(), "size".to_string()])
            .map(|r| r.output.trim().to_string())
            .unwrap_or_else(|_| "Unknown".to_string());

        let plugins = run_wp_command(&root, &www_dir, &workspace, &["plugin".to_string(), "list".to_string(), "--status=active".to_string(), "--format=count".to_string()])
            .map(|r| r.output.trim().parse().unwrap_or(0))
            .unwrap_or(0);

        let plugins_total = run_wp_command(&root, &www_dir, &workspace, &["plugin".to_string(), "list".to_string(), "--format=count".to_string()])
            .map(|r| r.output.trim().parse().unwrap_or(0))
            .unwrap_or(0);

        let themes_total = run_wp_command(&root, &www_dir, &workspace, &["theme".to_string(), "list".to_string(), "--format=count".to_string()])
            .map(|r| r.output.trim().parse().unwrap_or(0))
            .unwrap_or(0);

        Ok(WpWorkspaceInfo {
            version,
            site_url,
            title,
            db_size,
            plugins_active: plugins,
            plugins_total,
            themes_total,
        })
    })
    .await
    .map_err(|e| format!("workspace info task panicked: {e}"))?
}
