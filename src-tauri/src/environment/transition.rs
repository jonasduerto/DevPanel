use std::path::Path;

use crate::workspace::{vhost, Workspace};

use super::StackDefinition;

/// Regenerates every workspace's vhost/server-block for the newly active
/// stack (or the newly configured HTTP port), using each project's
/// portable `workspace.json` manifest — a workspace created under Apache
/// keeps serving unchanged after switching to Nginx, with no recreation
/// needed. Failures are collected as warnings rather than aborting the
/// switch partway through.
pub fn on_stack_changed(
    new_stack: &StackDefinition,
    workspaces: &[Workspace],
    root: &Path,
    www_dir: &str,
    http_port: u16,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for workspace in workspaces {
        if let Err(e) = vhost::regenerate(root, www_dir, workspace, new_stack, http_port) {
            warnings.push(format!("{}: {e}", workspace.id));
        }
    }
    warnings
}
