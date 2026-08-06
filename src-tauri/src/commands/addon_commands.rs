use std::collections::{BTreeMap, HashMap};
use std::os::windows::process::CommandExt;
use std::process::Command;

use tauri::State;

use crate::addons::{AddonActionWarning, AddonInventoryItem, AddonState};
use crate::config::PortConfig;
use crate::service::types::ServiceStatus;
use crate::state::AppState;

#[tauri::command]
pub fn list_addons(
    state: State<'_, AppState>,
    force_refresh: Option<bool>,
) -> Vec<AddonInventoryItem> {
    let addon_mgr = state.addon_mgr.lock().unwrap();
    if force_refresh == Some(true) {
        addon_mgr.invalidate_static_cache();
    }
    let statuses: HashMap<String, ServiceStatus> = {
        let status_vec = tauri::async_runtime::block_on(state.service_mgr.all_statuses());
        status_vec.into_iter().collect()
    };
    let config: BTreeMap<String, AddonState> = tauri::async_runtime::block_on(async {
        let config = state.config.lock().await;
        config.get().addons.clone()
    });
    addon_mgr.inventory(&state.service_mgr, &config, &statuses)
}

#[tauri::command]
pub async fn enable_addon(
    addon_id: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<Vec<AddonActionWarning>, String> {
    // Phase 1: fetch data (async), then validate (sync — no .await while holding addon_mgr)
    let validation_result = if enabled {
        let current_states = {
            let config = state.config.lock().await;
            config.get().addons.clone()
        };
        let ports = {
            let config = state.config.lock().await;
            config.get().ports
        };
        let addon_mgr = state.addon_mgr.lock().unwrap();
        let mut warnings = addon_mgr.validate_enable(&addon_id, &current_states);
        warnings.extend(web_port_conflict_warnings(
            &addon_id,
            &ports,
            &current_states,
        ));
        if !warnings.is_empty() {
            Some(warnings)
        } else {
            None
        }
    } else {
        let workspaces = {
            let store = state.workspace_store.lock().await;
            store.list()
        };
        let addon_mgr = state.addon_mgr.lock().unwrap();
        let warnings = addon_mgr.validate_disable(&addon_id, &workspaces);
        if !warnings.is_empty() {
            Some(warnings)
        } else {
            None
        }
    };

    if let Some(warnings) = validation_result {
        return Ok(warnings);
    }

    // Phase 2: persist state (addon_mgr is already dropped)
    {
        let mut config = state.config.lock().await;
        let mut addon_state = config
            .get()
            .addons
            .get(&addon_id)
            .cloned()
            .unwrap_or_default();
        addon_state.enabled = enabled;
        config.set_addon_state(addon_id.clone(), addon_state)?;
    }

    // Phase 3: start or stop the service
    let service_id = state.addon_mgr.lock().unwrap().service_id(&addon_id);
    if enabled {
        if let Err(error) = state.service_mgr.start(&service_id).await {
            // Do not leave a broken module enabled. The caller receives the
            // actionable error and can install, repair, or choose another
            // runtime without a half-active configuration lingering behind.
            let _ = state.service_mgr.stop(&service_id).await;
            let mut config = state.config.lock().await;
            let mut addon_state = config
                .get()
                .addons
                .get(&addon_id)
                .cloned()
                .unwrap_or_default();
            addon_state.enabled = false;
            config.set_addon_state(addon_id.clone(), addon_state)?;
            return Err(format!(
                "{addon_id} could not start and was disabled safely. Repair or install the runtime, then try again. Details: {error}"
            ));
        }
    } else {
        let _ = state.service_mgr.stop(&service_id).await;
    }

    Ok(Vec::new())
}

fn web_port_conflict_warnings(
    addon_id: &str,
    ports: &PortConfig,
    current_states: &BTreeMap<String, AddonState>,
) -> Vec<AddonActionWarning> {
    let Some(port) = ports.web_port(addon_id) else {
        return vec![];
    };
    let other_id = match addon_id {
        "apache" => "nginx",
        "nginx" => "apache",
        _ => return vec![],
    };
    let other_enabled = current_states
        .get(other_id)
        .map(|state| state.enabled)
        .unwrap_or(false);
    let other_port = ports.web_port(other_id).unwrap_or_default();
    if other_enabled && port == other_port {
        vec![AddonActionWarning {
            message: format!(
                "Apache and Nginx are both configured for port {port}. Give one web server a different Bind Port before enabling both."
            ),
            affected_workspaces: vec![],
        }]
    } else {
        vec![]
    }
}

#[tauri::command]
pub async fn set_addon_dashboard_visibility(
    addon_id: String,
    visible: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut config = state.config.lock().await;
    let mut addon_state = config
        .get()
        .addons
        .get(&addon_id)
        .cloned()
        .unwrap_or_default();
    addon_state.show_on_dashboard = visible;
    config.set_addon_state(addon_id, addon_state)
}

#[tauri::command]
pub async fn start_addon(addon_id: String, state: State<'_, AppState>) -> Result<String, String> {
    let addon_state = {
        let config = state.config.lock().await;
        config.get().addons.get(&addon_id).cloned()
    };
    match addon_state {
        Some(s) if s.enabled => {
            let current_states = {
                let config = state.config.lock().await;
                config.get().addons.clone()
            };
            let ports = {
                let config = state.config.lock().await;
                config.get().ports
            };
            if let Some(warning) =
                web_port_conflict_warnings(&addon_id, &ports, &current_states).first()
            {
                return Err(warning.message.clone());
            }
            let service_id = state.addon_mgr.lock().unwrap().service_id(&addon_id);
            let result = state.service_mgr.start(&service_id).await?;
            Ok(format!("{:?}", result))
        }
        Some(_) => Err(format!(
            "Addon '{}' is disabled. Enable it first.",
            addon_id
        )),
        None => Err(format!("Addon '{}' not found in config.", addon_id)),
    }
}

#[tauri::command]
pub async fn stop_addon(addon_id: String, state: State<'_, AppState>) -> Result<String, String> {
    let service_id = state.addon_mgr.lock().unwrap().service_id(&addon_id);
    let result = state.service_mgr.stop(&service_id).await?;
    Ok(format!("{:?}", result))
}

#[tauri::command]
pub async fn restart_addon(addon_id: String, state: State<'_, AppState>) -> Result<String, String> {
    let (current_states, ports) = {
        let config = state.config.lock().await;
        (config.get().addons.clone(), config.get().ports)
    };
    if let Some(warning) = web_port_conflict_warnings(&addon_id, &ports, &current_states).first() {
        return Err(warning.message.clone());
    }
    let service_id = state.addon_mgr.lock().unwrap().service_id(&addon_id);
    let _ = state.service_mgr.stop(&service_id).await;
    let result = state.service_mgr.start(&service_id).await?;
    Ok(format!("{:?}", result))
}

#[tauri::command]
pub fn get_addon_states(state: State<'_, AppState>) -> BTreeMap<String, AddonState> {
    tauri::async_runtime::block_on(async {
        let config = state.config.lock().await;
        config.get().addons.clone()
    })
}

/// Installs a native Windows runtime only after the user explicitly requests
/// it from Modules. DevPanel never invokes this during detection or startup.
#[tauri::command]
pub async fn install_native_addon(
    addon_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let package_id = match addon_id.as_str() {
        "apache" => "Apache.HttpServer",
        "nginx" => "Nginx.Nginx",
        "mysql" => "Oracle.MySQL",
        "postgres" => "PostgreSQL.PostgreSQL",
        "php" => "PHP.PHP",
        "node" => "OpenJS.NodeJS.LTS",
        "python" => "Python.Python.3.13",
        _ => return Err(format!("Native installation is not available for '{addon_id}' yet.")),
    };
    let package_id = package_id.to_string();
    tokio::task::spawn_blocking(move || {
        let output = Command::new("winget.exe")
            .args([
                "install",
                "--id",
                &package_id,
                "--exact",
                "--source",
                "winget",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ])
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|error| format!("Could not start Windows Package Manager: {error}"))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if output.status.success() {
            Ok(format!("{addon_id} installed natively. Refresh Modules to detect it."))
        } else {
            Err(format!("Windows Package Manager could not install {addon_id}: {}", text.trim()))
        }
    })
    .await
    .map_err(|error| format!("Windows Package Manager task failed: {error}"))?
    .map(|message| {
        state.addon_mgr.lock().unwrap().invalidate_static_cache();
        message
    })
}
