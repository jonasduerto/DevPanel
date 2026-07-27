use std::sync::Mutex;

use tokio::sync::Mutex as TokioMutex;

use crate::addons::AddonManager;
use crate::config::ConfigManager;
use crate::service::ServiceManager;
use crate::workspace::WorkspaceStore;

pub struct AppState {
    pub service_mgr: ServiceManager,
    pub config: TokioMutex<ConfigManager>,
    pub workspace_store: TokioMutex<WorkspaceStore>,
    pub addon_mgr: Mutex<AddonManager>,
}

impl AppState {
    pub fn new(service_mgr: ServiceManager, config: ConfigManager) -> Self {
        Self {
            service_mgr,
            config: TokioMutex::new(config),
            workspace_store: TokioMutex::new(WorkspaceStore::new()),
            addon_mgr: Mutex::new(AddonManager::new()),
        }
    }
}
