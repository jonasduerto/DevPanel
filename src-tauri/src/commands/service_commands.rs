use crate::service::types::{ServiceDefinition, ServiceStatus};
use crate::state::AppState;

#[tauri::command]
pub fn get_services(state: tauri::State<AppState>) -> Vec<ServiceDefinition> {
    state.service_mgr.definitions()
}

#[tauri::command]
pub async fn get_service_statuses(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<(String, ServiceStatus)>, String> {
    Ok(state.service_mgr.all_statuses().await)
}

#[tauri::command]
pub async fn start_service(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<ServiceStatus, String> {
    log::info!("Starting service: {}", id);
    state.service_mgr.start(&id).await
}

#[tauri::command]
pub async fn stop_service(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<ServiceStatus, String> {
    log::info!("Stopping service: {}", id);
    state.service_mgr.stop(&id).await
}

#[tauri::command]
pub async fn restart_service(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<ServiceStatus, String> {
    log::info!("Restarting service: {}", id);
    state.service_mgr.stop(&id).await?;
    state.service_mgr.start(&id).await
}
