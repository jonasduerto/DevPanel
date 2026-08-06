use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::service::types::ServiceStatus;
use crate::service::ServiceManager;
use crate::workspace::Workspace;

use super::types::*;

/// How long static detection (binary presence, external installations) may be
/// served from cache before re-scanning. Runtime status is always computed
/// fresh. Scanning spawns processes (where.exe + version probes), so it is
/// expensive on Windows and must not run on every Modules open.
const STATIC_CACHE_TTL: Duration = Duration::from_secs(300);

struct StaticScanCache {
    available: bool,
    external_installations: Vec<ExternalInstallation>,
    at: Instant,
}

pub struct AddonManager {
    definitions: Vec<AddonDefinition>,
    static_cache: Mutex<HashMap<String, StaticScanCache>>,
}

impl AddonManager {
    pub fn new() -> Self {
        Self {
            definitions: Self::builtin_definitions(),
            static_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Drops the cached static scan so the next inventory re-detects binaries
    /// and external installations. Called after install/uninstall mutations.
    pub fn invalidate_static_cache(&self) {
        self.static_cache.lock().unwrap().clear();
    }

    #[allow(dead_code)]
    pub fn definitions(&self) -> &[AddonDefinition] {
        &self.definitions
    }

    #[allow(dead_code)]
    pub fn definition(&self, id: &str) -> Option<&AddonDefinition> {
        self.definitions.iter().find(|d| d.id == id)
    }

    pub fn inventory(
        &self,
        service_mgr: &ServiceManager,
        state: &BTreeMap<String, AddonState>,
        service_statuses: &HashMap<String, ServiceStatus>,
    ) -> Vec<AddonInventoryItem> {
        self.definitions
            .iter()
            .map(|def| {
                let addon_state = state.get(&def.id).cloned().unwrap_or_default();
                let (available, external_installations) = {
                    let mut cache = self.static_cache.lock().unwrap();
                    let fresh = cache
                        .get(&def.id)
                        .filter(|entry| entry.at.elapsed() < STATIC_CACHE_TTL);
                    match fresh {
                        Some(entry) => (entry.available, entry.external_installations.clone()),
                        None => {
                            let available = Self::is_available(service_mgr, &def.id);
                            let external_installations = Self::external_installations(&def.id);
                            cache.insert(
                                def.id.clone(),
                                StaticScanCache {
                                    available,
                                    external_installations: external_installations.clone(),
                                    at: Instant::now(),
                                },
                            );
                            (available, external_installations)
                        }
                    }
                };
                let running = service_statuses
                    .get(&def.id)
                    .map(|s| matches!(s, ServiceStatus::Running))
                    .unwrap_or(false);
                AddonInventoryItem {
                    definition: def.clone(),
                    state: addon_state,
                    external_installations,
                    available,
                    running,
                }
            })
            .collect()
    }

    /// The service ID used by ServiceManager for a given addon.
    /// Most addons map 1:1; only PHP has version-qualified service IDs.
    pub fn service_id(&self, addon_id: &str) -> String {
        addon_id.to_string()
    }

    /// Validate that an addon can be enabled. Port conflicts between web
    /// servers are checked by the command layer because they depend on the
    /// user's current port configuration.
    pub fn validate_enable(
        &self,
        addon_id: &str,
        current_states: &BTreeMap<String, AddonState>,
    ) -> Vec<AddonActionWarning> {
        let def = match self.definitions.iter().find(|d| d.id == addon_id) {
            Some(d) => d,
            None => return vec![],
        };

        let mut warnings = Vec::new();

        // Check conflicts: if any conflicting addon is already enabled, warn.
        for conflict_id in &def.conflicts {
            if let Some(state) = current_states.get(conflict_id) {
                if state.enabled {
                    let conflict_def = self.definitions.iter().find(|d| d.id == *conflict_id);
                    let conflict_name =
                        conflict_def.map(|d| d.name.as_str()).unwrap_or(conflict_id);
                    warnings.push(AddonActionWarning {
                        message: format!(
                            "{} is already enabled and conflicts with {}. Disable {} first.",
                            conflict_name, def.name, conflict_name
                        ),
                        affected_workspaces: vec![],
                    });
                }
            }
        }

        warnings
    }

    /// Validate that an addon can be disabled.
    /// Checks if any running workspace depends on this addon.
    pub fn validate_disable(
        &self,
        addon_id: &str,
        workspaces: &[Workspace],
    ) -> Vec<AddonActionWarning> {
        let mut warnings = Vec::new();
        let mut affected: Vec<String> = Vec::new();

        let is_db_addon = addon_id == "mysql" || addon_id == "postgres";
        let is_web_addon = addon_id == "apache" || addon_id == "nginx";

        for ws in workspaces {
            let depends = if is_web_addon {
                true // All running sites need a web server
            } else if is_db_addon {
                let preset = ws.preset.as_str().to_lowercase();
                (addon_id == "mysql" && preset.contains("wordpress"))
                    || (addon_id == "postgres" && preset.contains("postgres"))
            } else {
                false
            };
            if depends && ws.running {
                affected.push(ws.name.clone());
            }
        }

        if !affected.is_empty() {
            warnings.push(AddonActionWarning {
                message: format!(
                    "{} running site(s) depend on this addon and will be stopped.",
                    affected.len()
                ),
                affected_workspaces: affected,
            });
        }

        warnings
    }

    pub fn is_available(service_mgr: &ServiceManager, id: &str) -> bool {
        match id {
            "apache" => service_mgr.find_binary("apache", "httpd.exe").is_some(),
            "nginx" => service_mgr.find_binary("nginx", "nginx.exe").is_some(),
            "mysql" => service_mgr.find_binary("mysql", "mysqld.exe").is_some(),
            "postgres" => service_mgr
                .find_binary("postgres", "postgres.exe")
                .is_some(),
            "php" => {
                service_mgr.find_binary("php", "php-cgi.exe").is_some()
                    || service_mgr.find_binary("php", "php.exe").is_some()
            }
            "node" => service_mgr.find_binary("node", "node.exe").is_some(),
            "python" => service_mgr.find_binary("python", "python.exe").is_some(),
            "redis" => service_mgr
                .find_binary("redis", "redis-server.exe")
                .is_some(),
            "mailpit" => service_mgr.find_binary("sendmail", "mailpit.exe").is_some(),
            _ => false,
        }
    }

    fn external_installations(id: &str) -> Vec<ExternalInstallation> {
        let executable = match id {
            "apache" => "httpd.exe",
            "nginx" => "nginx.exe",
            "mysql" => "mysqld.exe",
            "postgres" => "postgres.exe",
            "php" => "php.exe",
            "node" => "node.exe",
            "python" => "python.exe",
            "redis" => "redis-server.exe",
            "mailpit" => "mailpit.exe",
            _ => return vec![],
        };
        crate::service::find_external_installations(executable)
            .into_iter()
            .map(|path| ExternalInstallation {
                version: crate::service::binary_version(&path),
                path: path.to_string_lossy().into_owned(),
            })
            .collect()
    }

    /// Migrate from the legacy `active_stack_id` to the addon model.
    pub fn migrate_from_stack(stack_id: &str) -> BTreeMap<String, AddonState> {
        let mut addons = BTreeMap::new();
        let enabled_ids: &[&str] = match stack_id {
            "apache-mariadb-php" => &["mysql", "php", "apache"],
            "nginx-postgres-node" => &["postgres", "node", "nginx"],
            "apache-nginx-proxy-mariadb-php" => &["mysql", "php", "apache", "nginx"],
            _ => &[],
        };
        for id in enabled_ids {
            addons.insert(
                (*id).to_string(),
                AddonState {
                    enabled: true,
                    show_on_dashboard: *id == "apache" || *id == "nginx" || *id == "mysql",
                    ..Default::default()
                },
            );
        }
        addons
    }

    fn builtin_definitions() -> Vec<AddonDefinition> {
        vec![
            AddonDefinition {
                id: "apache".into(),
                name: "Apache".into(),
                description: "Apache HTTP Server".into(),
                category: AddonCategory::WebServer,
                dependencies: vec![],
                conflicts: vec![],
                dashboard_capable: true,
            },
            AddonDefinition {
                id: "nginx".into(),
                name: "Nginx".into(),
                description: "Nginx Web Server".into(),
                category: AddonCategory::WebServer,
                dependencies: vec![],
                conflicts: vec![],
                dashboard_capable: true,
            },
            AddonDefinition {
                id: "mysql".into(),
                name: "MySQL".into(),
                description: "MySQL / MariaDB Database".into(),
                category: AddonCategory::Database,
                dependencies: vec![],
                conflicts: vec![],
                dashboard_capable: true,
            },
            AddonDefinition {
                id: "postgres".into(),
                name: "PostgreSQL".into(),
                description: "PostgreSQL Database".into(),
                category: AddonCategory::Database,
                dependencies: vec![],
                conflicts: vec![],
                dashboard_capable: true,
            },
            AddonDefinition {
                id: "php".into(),
                name: "PHP".into(),
                description: "PHP Runtime".into(),
                category: AddonCategory::Runtime,
                dependencies: vec![],
                conflicts: vec![],
                dashboard_capable: false,
            },
            AddonDefinition {
                id: "node".into(),
                name: "Node.js".into(),
                description: "Node.js Runtime".into(),
                category: AddonCategory::Runtime,
                dependencies: vec![],
                conflicts: vec![],
                dashboard_capable: false,
            },
            AddonDefinition {
                id: "python".into(),
                name: "Python".into(),
                description: "Python Runtime".into(),
                category: AddonCategory::Runtime,
                dependencies: vec![],
                conflicts: vec![],
                dashboard_capable: false,
            },
            AddonDefinition {
                id: "redis".into(),
                name: "Redis".into(),
                description: "Redis Cache".into(),
                category: AddonCategory::Cache,
                dependencies: vec![],
                conflicts: vec![],
                dashboard_capable: true,
            },
            AddonDefinition {
                id: "mailpit".into(),
                name: "Mailpit".into(),
                description: "Catches local outgoing mail for testing".into(),
                category: AddonCategory::Tool,
                dependencies: vec![],
                conflicts: vec![],
                dashboard_capable: true,
            },
        ]
    }
}
