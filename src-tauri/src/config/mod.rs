use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn default_tld() -> String {
    ".test".into()
}

fn default_preferred_editor() -> String {
    "vscode".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub mysql_version: Option<String>,
    /// Relative to the portable app root, so the whole tree can move together.
    pub www_dir: Option<String>,
    pub panel_hotkey: Option<String>,
    /// Id of the stack the main panel starts/stops as a unit.
    /// Only changed from Settings — the main panel never re-prompts for it.
    pub active_stack_id: Option<String>,
    /// Local domain suffix new (and renamed) workspaces get, e.g. ".test".
    #[serde(default = "default_tld")]
    pub tld: String,
    /// Editor opened by the primary IDE shortcut on every site card.
    #[serde(default = "default_preferred_editor")]
    pub preferred_editor: String,
    #[serde(default)]
    pub ports: PortConfig,
    #[serde(default)]
    pub show_recovery_in_dashboard: bool,
    /// Per-addon user preference (enabled, dashboard visibility, version).
    /// Persisted independently — the binary discovery is always dynamic.
    #[serde(default)]
    pub addons: BTreeMap<String, crate::addons::AddonState>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mysql_version: None,
            www_dir: Some("www".into()),
            panel_hotkey: Some("Ctrl+Shift+D".into()),
            active_stack_id: Some("apache-mariadb-php".into()),
            tld: default_tld(),
            preferred_editor: default_preferred_editor(),
            ports: PortConfig::default(),
            show_recovery_in_dashboard: false,
            addons: BTreeMap::new(),
        }
    }
}

/// Ports services bind to, overriding their upstream defaults — lets the
/// user dodge conflicts with other software already using 80/3306/etc.
/// Web servers deliberately have independent ports: Apache and Nginx can run
/// together as long as they do not attempt to bind the same one. Changes only
/// take effect the next time a service is (re)started.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PortConfig {
    /// Apache's own HTTP listener. `http` is accepted when loading a legacy
    /// config so existing installations keep their previous Apache port.
    #[serde(default = "default_apache_port", alias = "http")]
    pub apache: u16,
    /// Nginx's own HTTP listener. A new install uses 8080 so Apache can keep
    /// the conventional port 80 without blocking Nginx.
    #[serde(default = "default_nginx_port")]
    pub nginx: u16,
    pub mysql: u16,
    pub postgres: u16,
    pub redis: u16,
}

impl Default for PortConfig {
    fn default() -> Self {
        Self {
            apache: default_apache_port(),
            nginx: default_nginx_port(),
            mysql: 3306,
            postgres: 5432,
            redis: 6379,
        }
    }
}

fn default_apache_port() -> u16 {
    80
}

fn default_nginx_port() -> u16 {
    8080
}

impl PortConfig {
    /// The browser-facing port for the selected environment. Nginx owns the
    /// public edge in both direct-Nginx and reverse-proxy environments.
    pub fn public_http_port(&self, stack: &crate::environment::StackDefinition) -> u16 {
        match &stack.web_role {
            crate::environment::WebRole::Direct(service) if service == "nginx" => self.nginx,
            crate::environment::WebRole::ReverseProxy { .. } => self.nginx,
            _ => self.apache,
        }
    }

    pub fn web_port(&self, addon_id: &str) -> Option<u16> {
        match addon_id {
            "apache" => Some(self.apache),
            "nginx" => Some(self.nginx),
            _ => None,
        }
    }
}

pub struct ConfigManager {
    path: PathBuf,
    config: AppConfig,
    connection: Connection,
}

impl ConfigManager {
    pub fn new() -> Self {
        let directory = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("devpanel");
        let path = directory.join("config.json");
        let _ = fs::create_dir_all(&directory);
        let connection = Connection::open(directory.join("devpanel.sqlite"))
            .unwrap_or_else(|_| Connection::open_in_memory().expect("in-memory SQLite must open"));
        let _ = connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS app_state (key TEXT PRIMARY KEY, payload TEXT NOT NULL);",
        );

        let mut config = connection
            .query_row(
                "SELECT payload FROM app_state WHERE key = 'config'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
            .and_then(|payload| serde_json::from_str(&payload).ok())
            .unwrap_or_else(|| {
                if path.exists() {
                    fs::read_to_string(&path)
                        .ok()
                        .and_then(|c| serde_json::from_str(&c).ok())
                        .unwrap_or_default()
                } else {
                    AppConfig::default()
                }
            });
        // Configurations written before stacks became the sole runtime model
        // do not have an active stack id. Migrate them to the canonical
        // default instead of preserving an ambiguous `None` state.
        if config.active_stack_id.is_none() {
            config.active_stack_id = Some(crate::environment::DEFAULT_STACK_ID.into());
        }
        // Migrate legacy stack to addon model if addons map is empty.
        if config.addons.is_empty() {
            if let Some(stack_id) = &config.active_stack_id {
                config.addons = crate::addons::AddonManager::migrate_from_stack(stack_id);
            }
        }
        if let Ok(payload) = serde_json::to_string(&config) {
            let _ = connection.execute(
                "INSERT OR IGNORE INTO app_state (key, payload) VALUES ('config', ?1)",
                params![payload],
            );
        }

        Self {
            path,
            config,
            connection,
        }
    }

    pub fn get(&self) -> &AppConfig {
        &self.config
    }

    pub fn set_active_stack(&mut self, stack_id: Option<String>) -> Result<(), String> {
        self.config.active_stack_id = stack_id;
        self.save()
    }

    pub fn set_tld(&mut self, tld: String) -> Result<(), String> {
        self.config.tld = tld;
        self.save()
    }

    pub fn set_preferred_editor(&mut self, editor: String) -> Result<(), String> {
        self.config.preferred_editor = editor;
        self.save()
    }

    pub fn set_ports(&mut self, ports: PortConfig) -> Result<(), String> {
        self.config.ports = ports;
        self.save()
    }

    pub fn set_mysql_version(&mut self, version: Option<String>) -> Result<(), String> {
        self.config.mysql_version = version;
        self.save()
    }

    pub fn set_show_recovery_in_dashboard(&mut self, enabled: bool) -> Result<(), String> {
        self.config.show_recovery_in_dashboard = enabled;
        self.save()
    }

    pub fn set_addon_state(
        &mut self,
        addon_id: String,
        state: crate::addons::AddonState,
    ) -> Result<(), String> {
        self.config.addons.insert(addon_id, state);
        self.save()
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(&self.config).map_err(|e| e.to_string())?;
        self.connection.execute(
            "INSERT INTO app_state (key, payload) VALUES ('config', ?1) ON CONFLICT(key) DO UPDATE SET payload = excluded.payload",
            params![json],
        ).map_err(|error| error.to_string())?;
        let json = serde_json::to_string_pretty(&self.config).map_err(|e| e.to_string())?;
        fs::write(&self.path, json).map_err(|e| e.to_string())?;
        Ok(())
    }
}
