use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use super::types::WorkspacePreset;

/// The portable, webserver-agnostic description of a project — persisted as
/// `workspace.json` inside the project folder itself (not in DevPanel's own
/// registry), so it travels with the project and survives an Environment
/// switch without needing the workspace recreated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub id: String,
    pub domain: String,
    pub preset: WorkspacePreset,
    pub php_version: Option<String>,
    /// Relative to the project folder — e.g. "public" for Laravel, "" for
    /// WordPress/PHP/other starter sites (served from the project root).
    pub doc_root: String,
    pub ssl_enabled: bool,
    /// Absolute paths to the leaf cert/key issued by the local CA, set
    /// once `finish_domain_setup` has run. Relied on by renderers when
    /// `ssl_enabled` is true.
    pub ssl_cert_file: Option<String>,
    pub ssl_key_file: Option<String>,
}

impl WorkspaceManifest {
    pub fn load(project_dir: &Path) -> Result<Self, String> {
        let path = project_dir.join("workspace.json");
        let contents =
            fs::read_to_string(&path).map_err(|e| format!("Could not read workspace.json: {e}"))?;
        serde_json::from_str(&contents).map_err(|e| format!("Could not parse workspace.json: {e}"))
    }

    pub fn save(&self, project_dir: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(project_dir.join("workspace.json"), json).map_err(|e| e.to_string())
    }
}

pub fn default_doc_root(preset: &WorkspacePreset) -> &'static str {
    if preset.as_str().eq_ignore_ascii_case("laravel") {
        "public"
    } else {
        ""
    }
}
