use std::fs;
use std::path::Path;

use crate::environment::StackDefinition;
use crate::ssl::hosts::{self, HostsOp};
use crate::ssl::CertificateAuthority;

use super::manifest::WorkspaceManifest;
use super::scaffold::project_path;
use super::vhost;
use super::Workspace;

/// Recomputes every workspace's domain for a new TLD (`{id}{tld}`),
/// reissuing its SSL cert and swapping its hosts entry if HTTPS was
/// already set up, and regenerating its vhost. All hosts-file edits are
/// batched into one elevated call regardless of how many workspaces
/// change, so switching TLD never triggers more than one UAC prompt.
/// Returns (warnings, workspaces that actually changed).
pub fn rename_all(
    root: &Path,
    www_dir: &str,
    workspaces: &[Workspace],
    new_tld: &str,
    stack: Option<&StackDefinition>,
    http_port: u16,
) -> (Vec<String>, Vec<Workspace>) {
    let mut warnings = Vec::new();
    let mut updated = Vec::new();
    let mut hosts_ops: Vec<(HostsOp, String)> = Vec::new();

    let needs_ca = workspaces.iter().any(|w| w.https_ready);
    let ca = if needs_ca {
        match CertificateAuthority::load_or_create(root) {
            Ok(ca) => Some(ca),
            Err(e) => {
                warnings.push(format!("Could not load local CA, certs not reissued: {e}"));
                None
            }
        }
    } else {
        None
    };

    for ws in workspaces {
        let new_domain = format!("{}{}", ws.id, new_tld);
        if new_domain == ws.domain {
            continue;
        }

        let project_dir = project_path(root, www_dir, &ws.id);
        let mut manifest = match WorkspaceManifest::load(&project_dir) {
            Ok(m) => m,
            Err(e) => {
                warnings.push(format!("{}: {e}", ws.id));
                continue;
            }
        };

        let old_domain = ws.domain.clone();
        manifest.domain = new_domain.clone();

        if ws.https_ready {
            if let Some(ca) = &ca {
                match ca.issue_cert(&new_domain, &project_dir.join("ssl")) {
                    Ok(cert) => {
                        if let Some(old_cert) = &manifest.ssl_cert_file {
                            let _ = fs::remove_file(old_cert);
                        }
                        if let Some(old_key) = &manifest.ssl_key_file {
                            let _ = fs::remove_file(old_key);
                        }
                        manifest.ssl_cert_file =
                            Some(cert.cert_file.to_string_lossy().into_owned());
                        manifest.ssl_key_file = Some(cert.key_file.to_string_lossy().into_owned());
                    }
                    Err(e) => warnings.push(format!("{}: could not reissue cert: {e}", ws.id)),
                }
            }
            hosts_ops.push((HostsOp::Remove, old_domain));
            hosts_ops.push((HostsOp::Add, new_domain.clone()));
        }

        if let Err(e) = manifest.save(&project_dir) {
            warnings.push(format!("{}: {e}", ws.id));
        }

        if let Some(stack) = stack {
            if let Err(e) = vhost::regenerate(root, www_dir, ws, stack, http_port) {
                warnings.push(format!("{}: vhost not regenerated: {e}", ws.id));
            }
        }

        let mut ws = ws.clone();
        ws.domain = new_domain;
        updated.push(ws);
    }

    if let Err(e) = hosts::apply_batch(&hosts_ops) {
        warnings.push(e);
    }

    (warnings, updated)
}
