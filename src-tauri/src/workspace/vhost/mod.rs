mod apache;
mod nginx;
mod templates;

use std::fs;
use std::path::{Path, PathBuf};

use crate::environment::{StackDefinition, WebRole};

use super::manifest::WorkspaceManifest;
use super::scaffold::{metadata_path, workspace_path};
use super::Workspace;

pub use apache::ApacheVhostRenderer;
pub use nginx::{NginxDirectRenderer, NginxProxyRenderer};

/// Single source of truth for where a generated vhost lives: DevPanel's own
/// `data/` tree, keyed by engine — never inside `bin/<engine>/`, which is
/// binaries only. The underlying source of truth for a site is its
/// `workspace.json` manifest; this is just where the *compiled* output for
/// one engine goes so Apache/Nginx/whatever can include it.
pub(super) fn generated_vhost_path(root: &Path, engine: &str, id: &str) -> PathBuf {
    root.join("data")
        .join("vhosts")
        .join(engine)
        .join(format!("{id}.conf"))
}

/// Apache and Nginx on Windows do not understand the Win32 extended-length
/// prefix (`\\?\`). Rust may preserve that prefix after canonicalisation,
/// which otherwise produces a valid-looking vhost that Apache answers with
/// 403. Web-server configuration always needs a normal forward-slash path.
pub(super) fn web_server_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    value
        .strip_prefix("//?/")
        .map(str::to_owned)
        .unwrap_or(value)
}

pub trait VhostRenderer {
    /// `is_public` tells the renderer whether this is the internet/LAN
    /// -facing render (gets the HTTPS sibling block when SSL is enabled)
    /// or an internal one (e.g. the Apache backend behind an Nginx
    /// reverse proxy, which never terminates TLS itself).
    fn render(
        &self,
        root: &Path,
        manifest: &WorkspaceManifest,
        project_dir: &Path,
        listen_port: u16,
        is_public: bool,
    ) -> String;
    fn config_path(&self, root: &Path, id: &str) -> PathBuf;
}

/// Which renderer(s) a stack needs, paired with the port each one listens
/// on and whether it's the public-facing one. A reverse-proxy stack needs
/// two: Apache on an internal backend port, Nginx public-facing on
/// `http_port` forwarding to it.
pub fn renderers_for_stack(
    stack: &StackDefinition,
    http_port: u16,
) -> Vec<(Box<dyn VhostRenderer>, u16, bool)> {
    match &stack.web_role {
        WebRole::Direct(service) if service == "apache" => {
            vec![(
                Box::new(ApacheVhostRenderer) as Box<dyn VhostRenderer>,
                http_port,
                true,
            )]
        }
        WebRole::Direct(service) if service == "nginx" => {
            vec![(
                Box::new(NginxDirectRenderer) as Box<dyn VhostRenderer>,
                http_port,
                true,
            )]
        }
        WebRole::Direct(_) => vec![],
        WebRole::ReverseProxy { backend_port, .. } => vec![
            (
                Box::new(ApacheVhostRenderer) as Box<dyn VhostRenderer>,
                *backend_port,
                false,
            ),
            (
                Box::new(NginxProxyRenderer {
                    backend_port: *backend_port,
                }) as Box<dyn VhostRenderer>,
                http_port,
                true,
            ),
        ],
    }
}

/// Re-renders every vhost/server-block this workspace needs for `stack`,
/// sourced from the portable `workspace.json` manifest in its project
/// folder — independent of whichever web server was active when it was
/// created. Safe to call repeatedly (e.g. on every Environment or port
/// switch).
pub fn regenerate(
    root: &Path,
    www_dir: &str,
    workspace: &Workspace,
    stack: &StackDefinition,
    http_port: u16,
) -> Result<(), String> {
    let metadata_dir = metadata_path(root, www_dir, &workspace.id);
    let project_dir = workspace_path(root, www_dir, workspace);
    let manifest = WorkspaceManifest::load(&metadata_dir)?;

    for (renderer, port, is_public) in renderers_for_stack(stack, http_port) {
        let rendered = renderer.render(root, &manifest, &project_dir, port, is_public);
        let config_path = renderer.config_path(root, &workspace.id);
        if let Some(dir) = config_path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(&config_path, rendered).map_err(|e| e.to_string())?;
    }
    Ok(())
}
