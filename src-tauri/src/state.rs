use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::{oneshot, Mutex as TokioMutex};

use crate::addons::AddonManager;
use crate::config::ConfigManager;
use crate::service::ServiceManager;
use crate::workspace::WorkspaceStore;

pub struct AppState {
    pub service_mgr: ServiceManager,
    pub config: TokioMutex<ConfigManager>,
    pub workspace_store: TokioMutex<WorkspaceStore>,
    pub addon_mgr: Mutex<AddonManager>,
    /// Long-running dev/watch scripts (`bun run dev`, `composer run-script dev`…)
    /// started via `start_project_script`, keyed by `"{workspace_id}:{source}:{script}"`.
    /// Holds a kill signal only — the child process itself lives inside the task
    /// that owns it, so stopping never races with the task reading its output.
    pub running_scripts: TokioMutex<HashMap<String, oneshot::Sender<()>>>,
}

impl AppState {
    pub fn new(service_mgr: ServiceManager, config: ConfigManager) -> Self {
        Self {
            service_mgr,
            config: TokioMutex::new(config),
            workspace_store: TokioMutex::new(WorkspaceStore::new()),
            addon_mgr: Mutex::new(AddonManager::new()),
            running_scripts: TokioMutex::new(HashMap::new()),
        }
    }
}
