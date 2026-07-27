use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::manifest::{default_doc_root, WorkspaceManifest};
use super::types::{Workspace, WorkspacePreset};
use super::vhost;
use crate::environment::StackDefinition;
use crate::service::find_binary_in_bin;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Converts a display name into a filesystem/DB/domain-safe slug.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "workspace".into()
    } else {
        slug
    }
}

pub fn project_path(root: &Path, www_dir: &str, id: &str) -> PathBuf {
    root.join(www_dir).join(id)
}

/// Finds a companion CLI tool (wp-cli, composer, mysql client…) the same
/// way services are located: DevPanel's own {root}/bin/{dir}/{binary} only.
/// An external installation may be copied into this directory by an explicit
/// import action, but it is never used in-place.
pub(crate) fn find_tool(root: &Path, dir_name: &str, binary: &str) -> Option<PathBuf> {
    find_binary_in_bin(root, dir_name, binary)
}

/// Runs a companion CLI tool. `root` is used to put DevPanel's own PHP on
/// PATH first — needed because tools like `composer.bat` shell out to
/// whatever `php` resolves to, and this DevPanel installation's PHP isn't
/// necessarily on the system PATH.
fn run_tool(root: &Path, bin: &Path, args: &[&str], cwd: &Path, label: &str, warnings: &mut Vec<String>) {
    let path = match find_binary_in_bin(root, "php", "php.exe").and_then(|p| p.parent().map(Path::to_path_buf)) {
        Some(php_dir) => format!("{};{}", php_dir.display(), std::env::var("PATH").unwrap_or_default()),
        None => std::env::var("PATH").unwrap_or_default(),
    };
    let status = Command::new(bin)
        .args(args)
        .current_dir(cwd)
        .env("PATH", path)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => warnings.push(format!("{label} exited with status {s}")),
        Err(e) => warnings.push(format!("{label} failed to run: {e}")),
    }
}

fn write_notes(dir: &Path, title: &str, body: &str) -> Result<(), String> {
    fs::write(
        dir.join("NOTES.txt"),
        format!("{title} workspace\n\n{body}\n"),
    )
    .map_err(|e| e.to_string())
}

fn scaffold_preset(
    root: &Path,
    project_dir: &Path,
    preset: &WorkspacePreset,
) -> Result<Vec<String>, String> {
    fs::create_dir_all(project_dir).map_err(|e| format!("Could not create project folder: {e}"))?;

    let mut warnings = Vec::new();

    match preset.as_str().to_ascii_lowercase().as_str() {
        "empty" => {
            fs::write(
                project_dir.join("index.php"),
                "<?php\n// Empty workspace scaffolded by DevPanel\n",
            )
            .map_err(|e| e.to_string())?;
        }
        "wordpress" => {
            // WordPress's files and administrator are installed by the guided
            // create-site command after the database exists.
        }
        "laravel" => {
            if let Some(composer) = find_tool(root, "composer", "composer.bat") {
                run_tool(
                    root,
                    &composer,
                    &["create-project", "laravel/laravel", "."],
                    project_dir,
                    "composer create-project",
                    &mut warnings,
                );
            } else {
                warnings.push(
                    "Composer not found in bin/composer or PATH — created an empty folder \
                     instead."
                        .into(),
                );
                write_notes(
                    project_dir,
                    "Laravel",
                    "Place your Laravel project here, or install Composer \
                     (bin/composer/composer.bat) so DevPanel can run \
                     `composer create-project laravel/laravel` automatically next time.",
                )?;
            }
        }
        "blesta" | "whmcs" => {
            let product = if preset.as_str().eq_ignore_ascii_case("blesta") {
                "Blesta"
            } else {
                "WHMCS"
            };
            warnings.push(format!(
                "{product} is licensed commercial software — DevPanel cannot download it \
                 automatically. Upload your licensed package into this folder."
            ));
            write_notes(
                project_dir,
                product,
                &format!(
                    "{product} requires a paid license. Extract your downloaded {product} \
                     package into this folder, then finish setup through its web installer."
                ),
            )?;
        }
        custom => {
            write_notes(
                project_dir,
                custom,
                "This custom preset was defined in site-presets.conf. Place or scaffold the project files here.",
            )?;
        }
    }

    Ok(warnings)
}

/// Creates the project folder, scaffolds the chosen preset (best-effort —
/// missing tools become warnings, not hard failures), writes the portable
/// `workspace.json` manifest and renders the vhost(s) for `stack` (if one
/// is active yet). Returns informational warnings, not a hard error, so a
/// partially automated setup still leaves the workspace usable.
pub fn provision(
    root: &Path,
    www_dir: &str,
    workspace: &Workspace,
    stack: Option<&StackDefinition>,
    http_port: u16,
) -> Result<Vec<String>, String> {
    let project_dir = project_path(root, www_dir, &workspace.id);
    let mut warnings = scaffold_preset(root, &project_dir, &workspace.preset)?;

    let manifest = WorkspaceManifest {
        id: workspace.id.clone(),
        domain: workspace.domain.clone(),
        preset: workspace.preset.clone(),
        php_version: (!workspace.runtime_profile.php_version.eq_ignore_ascii_case("inherit"))
            .then(|| workspace.runtime_profile.php_version.clone()),
        doc_root: default_doc_root(&workspace.preset).to_string(),
        ssl_enabled: false,
        ssl_cert_file: None,
        ssl_key_file: None,
    };
    manifest.save(&project_dir)?;

    match stack {
        Some(stack) => {
            if let Err(e) = vhost::regenerate(root, www_dir, &workspace.id, stack, http_port) {
                warnings.push(format!("Vhost config not written: {e}"));
            }
        }
        None => warnings.push(
            "No active Environment selected yet — pick one in Settings to generate this \
             workspace's vhost."
                .into(),
        ),
    }

    Ok(warnings)
}

/// Best-effort `CREATE DATABASE`. Fails softly (returns Err as a message,
/// not a panic) when MySQL isn't running yet — the caller surfaces this as
/// a retryable warning instead of blocking workspace creation.
pub fn prepare_database(root: &Path, db_name: &str) -> Result<(), String> {
    prepare_database_with_password(root, db_name, "")
}

pub fn prepare_database_with_password(
    root: &Path,
    db_name: &str,
    root_password: &str,
) -> Result<(), String> {
    let mysql_client = find_tool(root, "mysql", "mysql.exe").ok_or_else(|| {
        "MySQL client (mysql.exe) not found — start MySQL and ensure bin/mysql/mysql.exe exists, \
         then retry."
            .to_string()
    })?;

    let mut command = Command::new(&mysql_client);
    command
        .args([
            "-u",
            "root",
            "-e",
            &format!("CREATE DATABASE IF NOT EXISTS `{db_name}`;"),
        ])
        .creation_flags(CREATE_NO_WINDOW);
    if !root_password.is_empty() {
        command.env("MYSQL_PWD", root_password);
    }
    let output = command
        .output()
        .map_err(|e| format!("Failed to run mysql client: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "mysql client exited with an error: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

pub fn delete_data(root: &Path, workspace: &Workspace) -> Result<(), String> {
    let Some(mysql_client) = find_tool(root, "mysql", "mysql.exe") else {
        return Ok(());
    };
    let output = Command::new(&mysql_client)
        .args([
            "-u",
            "root",
            "-e",
            &format!("DROP DATABASE IF EXISTS `{}`;", workspace.db_name),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to run mysql client: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "mysql client exited with an error: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

pub fn delete_config(root: &Path, workspace: &Workspace) -> Result<(), String> {
    // A workspace may have had either renderer active over its lifetime
    // (Environment switches regenerate in place) — clear both locations.
    for engine in ["apache", "nginx"] {
        let path = vhost::generated_vhost_path(root, engine, &workspace.id);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| format!("Could not delete vhost config: {e}"))?;
        }
    }
    Ok(())
}

pub fn delete_all(root: &Path, www_dir: &str, workspace: &Workspace) -> Result<(), String> {
    delete_config(root, workspace)?;
    // Best-effort: if MySQL isn't running there's no live database to drop
    // in the first place, and the whole workspace is about to be removed
    // anyway — a connection failure here must not block deleting the
    // actual project files (that was the bug: users had to start MySQL
    // just to be able to delete a broken/incomplete workspace).
    let _ = delete_data(root, workspace);
    let project_dir = project_path(root, www_dir, &workspace.id);
    if project_dir.exists() {
        fs::remove_dir_all(&project_dir)
            .map_err(|e| format!("Could not delete project folder: {e}"))?;
    }
    // Best-effort, same reasoning as delete_data above — an elevation
    // prompt the user dismisses (or a missing entry) must not block
    // deleting the workspace itself.
    let _ = crate::ssl::hosts::remove_entry(&workspace.domain);
    Ok(())
}

pub fn uninstall_keep_data(
    root: &Path,
    www_dir: &str,
    workspace: &Workspace,
) -> Result<(), String> {
    delete_config(root, workspace)?;
    let project_dir = project_path(root, www_dir, &workspace.id);
    if project_dir.exists() {
        fs::remove_dir_all(&project_dir)
            .map_err(|error| format!("Could not uninstall project files: {error}"))?;
    }
    let _ = crate::ssl::hosts::remove_entry(&workspace.domain);
    Ok(())
}
