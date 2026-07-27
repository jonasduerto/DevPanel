use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AddonCategory {
    WebServer,
    Database,
    Runtime,
    Cache,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: AddonCategory,
    pub dependencies: Vec<String>,
    pub conflicts: Vec<String>,
    pub dashboard_capable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonState {
    pub enabled: bool,
    pub show_on_dashboard: bool,
    pub selected_version: Option<String>,
}

impl Default for AddonState {
    fn default() -> Self {
        Self {
            enabled: false,
            show_on_dashboard: true,
            selected_version: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonInventoryItem {
    pub definition: AddonDefinition,
    pub state: AddonState,
    pub available: bool,
    pub running: bool,
}

/// Warning returned when an enable/disable action triggers side effects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonActionWarning {
    pub message: String,
    pub affected_workspaces: Vec<String>,
}
