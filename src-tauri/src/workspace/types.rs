use serde::{Deserialize, Serialize};

/// A user-configurable site preset. Stored as a string so entries added to
/// `site-presets.conf` do not require recompiling DevPanel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct WorkspacePreset(pub String);

impl WorkspacePreset {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn setup_complete_by_default() -> bool {
    true
}

/// Requested runtime profile for one site. `inherit` means the shared default
/// PHP runtime. PHP is deliberately the only per-site runtime dimension: web
/// servers and databases belong to the active global stack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteRuntimeProfile {
    pub php_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordPressAdmin {
    pub username: String,
    pub password: String,
    pub email: String,
}

impl Default for SiteRuntimeProfile {
    fn default() -> Self {
        Self {
            php_version: "inherit".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub preset: WorkspacePreset,
    pub domain: String,
    pub db_name: String,
    pub created_at: u64,
    /// True once `finish_domain_setup` has issued a cert and wired the
    /// hosts entry for `domain`. Defaults to false for workspaces created
    /// before this field existed.
    #[serde(default)]
    pub https_ready: bool,
    /// Workspace lifecycle intent. Runtime services are shared, so stopping
    /// one workspace only stops them when no other workspace is running.
    #[serde(default)]
    pub running: bool,
    /// False while guided provisioning still needs a successful WordPress
    /// install. Older saved workspaces predate this field and are preserved
    /// as ready rather than being incorrectly marked incomplete.
    #[serde(default = "setup_complete_by_default")]
    pub setup_complete: bool,
    #[serde(default)]
    pub runtime_profile: SiteRuntimeProfile,
    #[serde(default)]
    pub wordpress_admin: Option<WordPressAdmin>,
}

pub struct WorkspaceBuilder {
    id: String,
    name: String,
    preset: WorkspacePreset,
    domain: String,
    db_name: String,
    created_at: Option<u64>,
    https_ready: bool,
    running: bool,
    setup_complete: Option<bool>,
    runtime_profile: Option<SiteRuntimeProfile>,
    wordpress_admin: Option<WordPressAdmin>,
}

impl WorkspaceBuilder {
    pub fn new(
        id: String,
        name: String,
        preset: WorkspacePreset,
        domain: String,
        db_name: String,
    ) -> Self {
        Self {
            id,
            name,
            preset,
            domain,
            db_name,
            created_at: None,
            https_ready: false,
            running: false,
            setup_complete: None,
            runtime_profile: None,
            wordpress_admin: None,
        }
    }

    pub fn running(mut self, running: bool) -> Self {
        self.running = running;
        self
    }

    pub fn setup_complete(mut self, setup_complete: bool) -> Self {
        self.setup_complete = Some(setup_complete);
        self
    }

    pub fn runtime_profile(mut self, runtime_profile: SiteRuntimeProfile) -> Self {
        self.runtime_profile = Some(runtime_profile);
        self
    }

    pub fn wordpress_admin(mut self, admin: Option<WordPressAdmin>) -> Self {
        self.wordpress_admin = admin;
        self
    }

    pub fn build(self) -> Workspace {
        Workspace {
            id: self.id,
            name: self.name,
            preset: self.preset,
            domain: self.domain,
            db_name: self.db_name,
            created_at: self.created_at.unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            }),
            https_ready: self.https_ready,
            running: self.running,
            setup_complete: self
                .setup_complete
                .unwrap_or_else(|| setup_complete_by_default()),
            runtime_profile: self.runtime_profile.unwrap_or_default(),
            wordpress_admin: self.wordpress_admin,
        }
    }
}

pub trait Controllable {
    fn start(&mut self);
    fn stop(&mut self);
    fn is_running(&self) -> bool;
}

impl Controllable for Workspace {
    fn start(&mut self) {
        self.running = true;
    }

    fn stop(&mut self) {
        self.running = false;
    }

    fn is_running(&self) -> bool {
        self.running
    }
}
