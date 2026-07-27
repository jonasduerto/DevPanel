use std::path::PathBuf;

use serde::Serialize;

use crate::db;
use crate::environment::{self, StackDefinition};
use crate::service::types::ServiceStatus;
use crate::state::AppState;
use crate::workspace::Workspace;

#[derive(Serialize)]
pub struct ServiceOutcome {
    pub id: String,
    pub status: Option<ServiceStatus>,
    pub error: Option<String>,
}

#[tauri::command]
pub fn get_stacks() -> Vec<StackDefinition> {
    environment::predefined_stacks()
}

#[tauri::command]
pub async fn get_active_stack(state: tauri::State<'_, AppState>) -> Result<Option<String>, String> {
    let config = state.config.lock().await;
    Ok(config.get().active_stack_id.clone())
}

/// Switches the active Environment: migrates DB data if the new stack uses
/// a different engine (dump on the old engine, swap, restore on the new
/// one), then regenerates every workspace's vhost to match. A project
/// created under Apache/MariaDB keeps its data and keeps serving, unchanged,
/// after switching to Nginx/PostgreSQL — no manual export/import, no
/// recreation. Returns warnings (e.g. a failed dump, a missing
/// workspace.json) rather than failing the whole switch.
#[tauri::command]
pub async fn set_active_stack(
    stack_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let new_stack = environment::find_stack(&stack_id)?;
    let old_stack = {
        let config = state.config.lock().await;
        config.get().active_stack_id.clone()
    }
    .and_then(|id| environment::find_stack(&id).ok());

    let root = state.service_mgr.root().clone();
    let workspaces = {
        let store = state.workspace_store.lock().await;
        store.list()
    };
    let mut warnings = Vec::new();

    let old_db = old_stack.as_ref().and_then(db::migration::db_service_of);
    let new_db = db::migration::db_service_of(&new_stack);
    if let Some(old_db_id) = old_db {
        if old_db != new_db {
            warnings.extend(
                migrate_database_engine(
                    &state,
                    &root,
                    &workspaces,
                    old_stack.as_ref().unwrap(),
                    &new_stack,
                    old_db_id,
                    new_db,
                )
                .await,
            );
        }
    }

    {
        let mut config = state.config.lock().await;
        config.set_active_stack(Some(stack_id))?;
    }
    crate::commands::workspace_commands::refresh_runtime_detection(state.clone()).await?;

    let (www_dir, http_port) = {
        let config = state.config.lock().await;
        (
            config.get().www_dir.clone().unwrap_or_else(|| "www".into()),
            config.get().ports.public_http_port(&new_stack),
        )
    };
    let new_stack_for_vhost = new_stack.clone();
    let vhost_warnings = tokio::task::spawn_blocking(move || {
        environment::transition::on_stack_changed(
            &new_stack_for_vhost,
            &workspaces,
            &root,
            &www_dir,
            http_port,
        )
    })
    .await
    .map_err(|e| format!("Vhost regeneration task panicked: {e}"))?;
    warnings.extend(vhost_warnings);

    Ok(warnings)
}

/// Dumps every workspace's DB from `old_db_id` (starting it briefly if it
/// wasn't already running), stops it, starts `new_db_id` and restores.
async fn migrate_database_engine(
    state: &tauri::State<'_, AppState>,
    root: &PathBuf,
    workspaces: &[Workspace],
    old_stack: &StackDefinition,
    new_stack: &StackDefinition,
    old_db_id: &'static str,
    new_db_id: Option<&'static str>,
) -> Vec<String> {
    let mut warnings = Vec::new();

    let was_running = matches!(
        state.service_mgr.status(old_db_id).await,
        ServiceStatus::Running
    );
    if !was_running {
        if let Err(e) = state.service_mgr.start(old_db_id).await {
            warnings.push(format!(
                "Could not start {old_db_id} to migrate its data: {e}"
            ));
            return warnings;
        }
    }

    let dump_result = {
        let old_stack = old_stack.clone();
        let new_stack = new_stack.clone();
        let workspaces = workspaces.to_vec();
        let root = root.clone();
        tokio::task::spawn_blocking(move || {
            db::migration::dump_all(&old_stack, &new_stack, &workspaces, &root)
        })
        .await
    };
    match dump_result {
        Ok(w) => warnings.extend(w),
        Err(e) => warnings.push(format!("Database dump task panicked: {e}")),
    }

    let _ = state.service_mgr.stop(old_db_id).await;

    let Some(new_db_id) = new_db_id else {
        return warnings; // new stack has no DB service — nothing to restore into
    };

    match state.service_mgr.start(new_db_id).await {
        Ok(_) => {
            let old_stack = old_stack.clone();
            let new_stack = new_stack.clone();
            let workspaces = workspaces.to_vec();
            let root = root.clone();
            let restore_result = tokio::task::spawn_blocking(move || {
                db::migration::restore_all(&old_stack, &new_stack, &workspaces, &root)
            })
            .await;
            match restore_result {
                Ok(w) => warnings.extend(w),
                Err(e) => warnings.push(format!("Database restore task panicked: {e}")),
            }
        }
        Err(e) => warnings.push(format!("Could not start {new_db_id} to restore data: {e}")),
    }

    warnings
}

#[tauri::command]
pub async fn start_stack(
    stack_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ServiceOutcome>, String> {
    let stack = environment::find_stack(&stack_id)?;
    let mut results = Vec::new();
    for service_id in &stack.services {
        let outcome = match state.service_mgr.start(service_id).await {
            Ok(status) => ServiceOutcome {
                id: service_id.clone(),
                status: Some(status),
                error: None,
            },
            Err(e) => ServiceOutcome {
                id: service_id.clone(),
                status: None,
                error: Some(e),
            },
        };
        results.push(outcome);
    }
    Ok(results)
}

#[tauri::command]
pub async fn stop_stack(
    stack_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ServiceOutcome>, String> {
    let stack = environment::find_stack(&stack_id)?;
    let mut results = Vec::new();
    for service_id in stack.services.iter().rev() {
        let outcome = match state.service_mgr.stop(service_id).await {
            Ok(status) => ServiceOutcome {
                id: service_id.clone(),
                status: Some(status),
                error: None,
            },
            Err(e) => ServiceOutcome {
                id: service_id.clone(),
                status: None,
                error: Some(e),
            },
        };
        results.push(outcome);
    }
    Ok(results)
}
