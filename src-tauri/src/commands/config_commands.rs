use std::net::TcpListener;
use std::process::Command;

use serde::Serialize;

use crate::config::{AppConfig, PortConfig};
use crate::environment;
use crate::state::AppState;
use crate::workspace::domain;

#[tauri::command]
pub async fn get_config(state: tauri::State<'_, AppState>) -> Result<AppConfig, String> {
    let config = state.config.lock().await;
    Ok(config.get().clone())
}

#[tauri::command]
pub async fn set_show_recovery_in_dashboard(
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut config = state.config.lock().await;
    config.set_show_recovery_in_dashboard(enabled)
}

/// Whether Windows' NTFS long-path support is on. Read-only, no elevation.
#[tauri::command]
pub async fn get_long_paths_enabled() -> Result<bool, String> {
    tokio::task::spawn_blocking(crate::ssl::elevate::long_paths_enabled)
        .await
        .map_err(|e| format!("Long paths check task panicked: {e}"))
}

/// Enables it via one elevated registry write (single UAC prompt). This is
/// the durable fix for WP-CLI (and anything else) hitting MAX_PATH on
/// deeply-nested extractions — DevPanel's own temp-dir shortening only
/// buys margin, it doesn't remove the ceiling.
#[tauri::command]
pub async fn enable_long_paths() -> Result<(), String> {
    tokio::task::spawn_blocking(crate::ssl::elevate::enable_long_paths_elevated)
        .await
        .map_err(|e| format!("Enable long paths task panicked: {e}"))?
}

const ALLOWED_TLDS: [&str; 4] = [".dp", ".dev", ".local", ".test"];

#[derive(Serialize)]
pub struct PortAvailability {
    pub port: u16,
    pub available: bool,
    pub detail: String,
}

/// Tests whether the local TCP ports DevPanel needs can be bound before a
/// runtime is configured to use them. This is intentionally read-only: it
/// never terminates the owning process or changes the Windows firewall.
#[tauri::command]
pub async fn check_port_availability(ports: PortConfig) -> Result<Vec<PortAvailability>, String> {
    tokio::task::spawn_blocking(move || {
        let mut unique_ports = vec![
            ports.apache,
            ports.nginx,
            ports.mysql,
            ports.postgres,
            ports.redis,
        ];
        unique_ports.sort_unstable();
        unique_ports.dedup();
        unique_ports.into_iter().map(probe_port).collect()
    })
    .await
    .map_err(|e| format!("Port preflight task panicked: {e}"))
}

/// Finds a free HTTP port for the second web server without touching an
/// existing listener. The UI uses this before enabling Apache and Nginx
/// together, starting with 8080 and moving upward only when necessary.
#[tauri::command]
pub async fn suggest_available_web_port(
    preferred_port: u16,
    reserved_port: u16,
) -> Result<u16, String> {
    tokio::task::spawn_blocking(move || {
        let start = preferred_port.max(1024);
        for candidate in start..=u16::MAX {
            if candidate != reserved_port && TcpListener::bind(("127.0.0.1", candidate)).is_ok() {
                return Ok(candidate);
            }
        }
        Err("No available TCP port was found for the web server.".into())
    })
    .await
    .map_err(|e| format!("Web port suggestion task panicked: {e}"))?
}

fn probe_port(port: u16) -> PortAvailability {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => {
            drop(listener);
            PortAvailability {
                port,
                available: true,
                detail: "Available".into(),
            }
        }
        Err(error) => PortAvailability {
            port,
            available: false,
            detail: port_owner(port).unwrap_or_else(|| format!("Unavailable: {error}")),
        },
    }
}

fn port_owner(port: u16) -> Option<String> {
    let output = Command::new("netstat")
        .args(["-ano", "-p", "tcp"])
        .output()
        .ok()?;
    let port_marker = format!(":{port}");
    let pid = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find(|line| line.contains(&port_marker) && line.contains("LISTENING"))?
        .split_whitespace()
        .last()?
        .to_string();

    let task = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    let task_stdout = String::from_utf8_lossy(&task.stdout);
    let first_line = task_stdout.lines().next()?.trim();
    let name = first_line
        .trim_matches('"')
        .split("\",\"")
        .next()
        .unwrap_or("unknown process");
    Some(format!("In use by {name} (PID {pid})"))
}

/// Switches the local domain suffix for every workspace at once: reissues
/// certs, swaps hosts entries (batched into a single UAC prompt) and
/// regenerates vhosts. Returns per-workspace warnings rather than failing
/// the whole switch.
#[tauri::command]
pub async fn set_tld(
    tld: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    if !ALLOWED_TLDS.contains(&tld.as_str()) {
        return Err(format!(
            "Unsupported TLD '{tld}' — choose one of {ALLOWED_TLDS:?}"
        ));
    }

    let old_tld = {
        let config = state.config.lock().await;
        config.get().tld.clone()
    };
    if old_tld == tld {
        return Ok(vec![]);
    }

    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    let active_stack = {
        let config = state.config.lock().await;
        config.get().active_stack_id.clone()
    }
    .and_then(|id| {
        environment::predefined_stacks()
            .into_iter()
            .find(|s| s.id == id)
    });
    let workspaces = {
        let store = state.workspace_store.lock().await;
        store.list()
    };
    let http_port = {
        let config = state.config.lock().await;
        active_stack
            .as_ref()
            .map(|stack| config.get().ports.public_http_port(stack))
            .unwrap_or(config.get().ports.apache)
    };

    let tld_for_blocking = tld.clone();
    let (warnings, updated) = tokio::task::spawn_blocking(move || {
        domain::rename_all(
            &root,
            &www_dir,
            &workspaces,
            &tld_for_blocking,
            active_stack.as_ref(),
            http_port,
        )
    })
    .await
    .map_err(|e| format!("Domain rename task panicked: {e}"))?;

    {
        let mut config = state.config.lock().await;
        config.set_tld(tld)?;
    }
    {
        let mut store = state.workspace_store.lock().await;
        for ws in updated {
            store.update(ws)?;
        }
    }

    Ok(warnings)
}

#[tauri::command]
pub async fn set_ports(
    ports: PortConfig,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    if [ports.apache, ports.nginx, ports.mysql, ports.postgres, ports.redis]
        .into_iter()
        .any(|port| port == 0)
    {
        return Err("Ports must be between 1 and 65535.".into());
    }
    {
        let mut config = state.config.lock().await;
        config.set_ports(ports)?;
    }

    crate::commands::workspace_commands::refresh_runtime_detection(state.clone()).await?;

    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    let active_stack = {
        let config = state.config.lock().await;
        config.get().active_stack_id.clone()
    }
    .and_then(|id| {
        environment::predefined_stacks()
            .into_iter()
            .find(|s| s.id == id)
    });
    let workspaces = {
        let store = state.workspace_store.lock().await;
        store.list()
    };

    let Some(stack) = active_stack else {
        return Ok(vec![
            "No active Environment selected — vhosts will pick up the new port next time one is \
             generated."
                .into(),
        ]);
    };

    let http_port = ports.public_http_port(&stack);
    tokio::task::spawn_blocking(move || {
        environment::transition::on_stack_changed(&stack, &workspaces, &root, &www_dir, http_port)
    })
    .await
    .map_err(|e| format!("Vhost regeneration task panicked: {e}"))
}
