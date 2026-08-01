use std::fs;
use std::os::windows::process::CommandExt;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{path::PathBuf, process::Command};

use serde::{Deserialize, Serialize};

use crate::db::{engine_by_name, DatabaseEngine};
use crate::environment;
use crate::service::find_binary_in_bin;
use crate::service::types::ServiceStatus;
use crate::state::AppState;
use crate::workspace::{scaffold, vhost};
use crate::workspace::{
    Controllable, SiteRuntimeProfile, WordPressAdmin, Workspace, WorkspaceBuilder, WorkspacePreset,
};

#[derive(Serialize)]
pub struct CreateWorkspaceResult {
    pub workspace: Workspace,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredWorkspaceFolder {
    pub name: String,
    pub path: String,
    pub suggested_preset: String,
    pub suggested_document_root: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSettingsInput {
    pub domain: String,
    #[serde(default)]
    pub document_root: String,
    #[serde(default)]
    pub db_name: String,
}

#[derive(Serialize)]
pub struct UpdateWorkspaceSettingsResult {
    pub workspace: Workspace,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaravelEnvironment {
    pub app_url: String,
    pub db_connection: String,
    pub db_host: String,
    pub db_port: String,
    pub db_database: String,
    pub db_username: String,
    /// DevPanel runs locally, so the workstation owner can inspect and edit
    /// the same project password that exists in the local .env file.
    #[serde(default)]
    pub db_password: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSiteOptions {
    #[serde(default = "default_project_mode")]
    pub project_mode: String,
    #[serde(default)]
    pub external_root: String,
    #[serde(default)]
    pub document_root: String,
    #[serde(default)]
    pub create_database: bool,
    #[serde(default = "default_wordpress_version")]
    pub wordpress_version: String,
    #[serde(default)]
    pub wordpress_admin_user: String,
    #[serde(default)]
    pub wordpress_admin_password: String,
    #[serde(default)]
    pub wordpress_admin_email: String,
    #[serde(default)]
    pub runtime_profile: SiteRuntimeProfile,
}

fn default_project_mode() -> String { "app".into() }

fn default_wordpress_version() -> String {
    "latest".into()
}

fn wordpress_options(options: Option<CreateSiteOptions>) -> CreateSiteOptions {
    options.unwrap_or(CreateSiteOptions {
        project_mode: default_project_mode(),
        external_root: String::new(),
        document_root: String::new(),
        create_database: false,
        wordpress_version: default_wordpress_version(),
        wordpress_admin_user: String::new(),
        wordpress_admin_password: String::new(),
        wordpress_admin_email: String::new(),
        runtime_profile: SiteRuntimeProfile::default(),
    })
}

fn ensure_wp_cli(root: &std::path::Path) -> Result<PathBuf, String> {
    if let Some(path) = scaffold::find_tool(root, "wp-cli", "wp-cli.phar") {
        return Ok(path);
    }
    let destination = root.join("bin/wp-cli/wp-cli.phar");
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let script = format!(
        "Invoke-WebRequest -UseBasicParsing -Uri 'https://raw.githubusercontent.com/wp-cli/builds/gh-pages/phar/wp-cli.phar' -OutFile '{}'",
        destination.to_string_lossy().replace('\'', "''")
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|error| format!("Could not download WP-CLI: {error}"))?;
    if !output.status.success() || !destination.is_file() {
        return Err(format!(
            "Could not install WP-CLI in DevPanel/bin/wp: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(destination)
}

/// Ensures `%SystemRoot%\System32` is on PATH for the WP-CLI child process
/// — that's where Windows 10 1803+ ships `tar.exe`, which WP-CLI shells out
/// to as a fallback when PharData extraction fails. Some environments
/// (customized shells, restricted launch contexts) don't reliably inherit
/// it from the parent process.
fn path_with_system32() -> String {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let system32 = format!("{system_root}\\System32");
    let current = std::env::var("PATH").unwrap_or_default();
    if current
        .split(';')
        .any(|p| p.eq_ignore_ascii_case(&system32))
    {
        current
    } else {
        format!("{current};{system32}")
    }
}

fn run_wp_cli_setup(
    root: &std::path::Path,
    project_dir: &std::path::Path,
    args: &[String],
) -> Result<String, String> {
    let php = find_binary_in_bin(root, "php", "php.exe")
        .ok_or_else(|| "PHP is not installed in DevPanel/bin/php.".to_string())?;
    let wp = ensure_wp_cli(root)?;
    // As short as possible: PharData stages extraction in a subfolder named
    // after the archive itself (e.g. "wp-cli-extract-tarball-<hash>-wp_<hash>.tar.gz"),
    // and modern WordPress core nests some vendor libraries several levels
    // deep — every character shaved off this prefix is margin against
    // Windows' path-length limits. Kept under `data/` so it stays covered
    // by the existing "data/" gitignore rule.
    let temp_dir = root.join("data/tmp");
    let cache_dir = root.join("data/wp-cli-cache");
    fs::create_dir_all(&temp_dir)
        .map_err(|error| format!("Could not create DevPanel temporary directory: {error}"))?;
    fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("Could not create DevPanel WP-CLI cache: {error}"))?;
    let output = Command::new(php)
        // A PHP distribution may have a sys_temp_dir inherited from another
        // local stack. This process-only override keeps WP-CLI extraction in
        // DevPanel even when php.ini points at Laragon or another app.
        .arg("-d")
        .arg(format!("sys_temp_dir={}", temp_dir.display()))
        .arg(wp)
        .args(args)
        // Explicit --path wins over any `path:` set in a project or global
        // wp-cli.yml (e.g. a leftover `~/.wp-cli/config.yml` from another
        // local stack pointing at that stack's own www folder) — without
        // this, WP-CLI silently targets whatever that stale config says
        // instead of this workspace's actual project folder.
        .arg(format!("--path={}", project_dir.display()))
        .current_dir(project_dir)
        // Do not inherit Laragon's temporary-folder configuration. WP-CLI
        // extracts archives only inside DevPanel-owned data directories.
        .env("TEMP", &temp_dir)
        .env("TMP", &temp_dir)
        .env("TMPDIR", &temp_dir)
        .env("WP_CLI_CACHE_DIR", &cache_dir)
        .env("PATH", path_with_system32())
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|error| format!("Could not run WP-CLI: {error}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(text)
    } else {
        Err(text.trim().to_string())
    }
}

fn wordpress_archive_urls(version: &str) -> [String; 2] {
    let archive = if version.eq_ignore_ascii_case("latest") {
        "latest".to_string()
    } else {
        format!("wordpress-{version}")
    };
    [
        format!("https://wordpress.org/{archive}.zip"),
        format!("https://wordpress.org/{archive}.tar.gz"),
    ]
}

fn download_wordpress_core(
    root: &std::path::Path,
    project_dir: &std::path::Path,
    version: &str,
) -> Result<(), String> {
    let archive_urls = wordpress_archive_urls(version);
    let mut failures = Vec::new();

    for archive_url in archive_urls {
        let args = vec![
            "core".into(),
            "download".into(),
            archive_url.clone(),
            "--force".into(),
        ];
        match run_wp_cli_setup(root, project_dir, &args) {
            Ok(_) => return Ok(()),
            Err(error) => failures.push(format!("{archive_url}: {error}")),
        }
    }

    Err(format!(
        "Could not download WordPress using ZIP or TAR.GZ. {}",
        failures.join("\n\n")
    ))
}

fn install_wordpress(
    root: &std::path::Path,
    project_dir: &std::path::Path,
    workspace: &Workspace,
    options: &CreateSiteOptions,
    engine: &dyn DatabaseEngine,
    database_user: &str,
    database_password: &str,
    mysql_port: u16,
) -> Result<(), String> {
    if options.wordpress_admin_user.trim().is_empty()
        || options.wordpress_admin_password.len() < 8
        || !options.wordpress_admin_email.contains('@')
    {
        return Err(
            "WordPress requires an admin username, an 8+ character password, and an email address."
                .into(),
        );
    }
    download_wordpress_core(root, project_dir, &options.wordpress_version)?;

    // Engine-specific pre-install setup (e.g. SQLite plugin download + db.php + wp-config.php)
    engine.before_wp_install(root, project_dir)?;

    // If the engine provides wp-config args (MySQL/Postgres), run wp core config.
    // SQLite generates wp-config.php inside before_wp_install, so it returns empty.
    let config_args = engine.wp_config_args(
        &workspace.db_name,
        database_user,
        database_password,
        mysql_port,
    );
    if !config_args.is_empty() {
        let mut args = Vec::with_capacity(config_args.len() + 2);
        args.push("core".into());
        args.push("config".into());
        args.extend(config_args);
        run_wp_cli_setup(root, project_dir, &args)?;
    }

    let url = format!("http://{}", workspace.domain);

    run_wp_cli_setup(
        root,
        project_dir,
        &[
            "core".into(),
            "install".into(),
            format!("--url={url}"),
            format!("--title={}", workspace.name),
            format!("--admin_user={}", options.wordpress_admin_user),
            format!("--admin_password={}", options.wordpress_admin_password),
            format!("--admin_email={}", options.wordpress_admin_email),
            "--skip-email".into(),
        ],
    )?;
    Ok(())
}

#[derive(Serialize)]
pub struct WorkspacePaths {
    pub site_path: String,
    pub php_ini_path: Option<String>,
    pub mysql_data_path: String,
    pub composer_config_path: Option<String>,
    pub redis_config_path: Option<String>,
    pub memcached_config_path: Option<String>,
    pub sendmail_path: Option<String>,
    pub heidisql_available: bool,
    pub cmder_available: bool,
    pub phpmyadmin_available: bool,
}

#[derive(Serialize)]
pub struct RuntimeChoice {
    pub value: String,
    pub label: String,
    pub installed: bool,
}

#[derive(Serialize)]
pub struct RuntimeCatalog {
    pub php_versions: Vec<RuntimeChoice>,
}

#[derive(Serialize)]
pub struct SitePresetChoice {
    pub value: String,
    pub label: String,
}

#[derive(Serialize)]
pub struct PhpExtension {
    pub name: String,
    pub file_name: String,
    pub enabled: bool,
    pub zend_extension: bool,
}

fn devpanel_php_ini(root: &std::path::Path) -> Result<PathBuf, String> {
    let php_root = find_binary_in_bin(root, "php", "php.exe")
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()));
    php_root
        .into_iter()
        .map(|path| path.join("php.ini"))
        .chain([root.join("bin/php/php.ini"), root.join("bin/php.ini")])
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "PHP.ini was not found in DevPanel/bin/php.".into())
}

fn php_extension_files(root: &std::path::Path) -> Result<Vec<String>, String> {
    let ext_dir = find_binary_in_bin(root, "php", "php.exe")
        .and_then(|path| path.parent().map(|parent| parent.join("ext")))
        .unwrap_or_else(|| root.join("bin/php/ext"));
    let entries = fs::read_dir(&ext_dir)
        .map_err(|error| format!("Could not read {}: {error}", ext_dir.display()))?;
    Ok(entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.to_ascii_lowercase().ends_with(".dll"))
        .collect())
}

fn extension_name_from_file(file_name: &str) -> String {
    file_name
        .trim_end_matches(".dll")
        .trim_start_matches("php_")
        .to_string()
}

/// `data/config/php-extensions.json` — the neutral, version-independent
/// record of which extensions the user has explicitly toggled, keyed by
/// extension name (not by DLL file, which differs per PHP version).
/// Reapplied to whichever PHP version is active by `apply_extension_overrides`,
/// so toggles survive switching versions instead of living only in that
/// version's own `php.ini`.
fn extension_overrides_path(root: &std::path::Path) -> PathBuf {
    root.join("data/config/php-extensions.json")
}

fn load_extension_overrides(root: &std::path::Path) -> std::collections::HashMap<String, bool> {
    fs::read_to_string(extension_overrides_path(root))
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

fn save_extension_overrides(
    root: &std::path::Path,
    overrides: &std::collections::HashMap<String, bool>,
) -> Result<(), String> {
    let path = extension_overrides_path(root);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(overrides).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

/// Rewrites the `extension=`/`zend_extension=` line for one DLL in the
/// active PHP version's `php.ini`. Shared by `set_php_extension` (one
/// extension, from the UI) and `apply_extension_overrides` (every
/// extension in the manifest, reapplied after a version switch).
fn write_extension_state(
    root: &std::path::Path,
    actual_file: &str,
    enabled: bool,
) -> Result<(), String> {
    let ini = devpanel_php_ini(root)?;
    let original = fs::read_to_string(&ini).map_err(|error| error.to_string())?;
    let backup = ini.with_extension("ini.bak");
    if !backup.exists() {
        fs::write(&backup, &original).map_err(|error| error.to_string())?;
    }
    let directive = if actual_file.to_ascii_lowercase().contains("xdebug") {
        "zend_extension"
    } else {
        "extension"
    };
    let mut replaced = false;
    let rewritten = original
        .lines()
        .map(|line| {
            if extension_line_matches(line, actual_file).is_some() {
                replaced = true;
                if enabled {
                    format!("{directive}={actual_file}")
                } else {
                    format!(";{directive}={actual_file}")
                }
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    let final_contents = if replaced {
        rewritten
    } else {
        format!(
            "{rewritten}\r\n{}{}={actual_file}\r\n",
            if enabled { "" } else { ";" },
            directive
        )
    };
    fs::write(&ini, final_contents).map_err(|e| e.to_string())
}

/// Reapplies every stored extension toggle onto whichever PHP version is
/// currently active. Called from `detect_services()` so switching PHP
/// versions carries the user's choices over instead of resetting to
/// whatever that version's own `php.ini` happens to ship with.
pub fn apply_extension_overrides(root: &std::path::Path) {
    let overrides = load_extension_overrides(root);
    if overrides.is_empty() {
        return;
    }
    let Ok(files) = php_extension_files(root) else {
        return;
    };
    for file in files {
        let name = extension_name_from_file(&file);
        if let Some(&enabled) = overrides.get(&name) {
            let _ = write_extension_state(root, &file, enabled);
        }
    }
}

const XDEBUG_MODES: [&str; 4] = ["off", "debug", "profile", "trace"];

/// `data/config/php-xdebug.json` — persisted Xdebug mode, independent of
/// which PHP version's `php.ini` is currently active. Mirrors
/// `extension_overrides_path`.
fn xdebug_override_path(root: &std::path::Path) -> PathBuf {
    root.join("data/config/php-xdebug.json")
}

fn xdebug_output_dir(root: &std::path::Path) -> PathBuf {
    root.join("data/xdebug")
}

fn load_xdebug_mode(root: &std::path::Path) -> String {
    fs::read_to_string(xdebug_override_path(root))
        .ok()
        .and_then(|contents| serde_json::from_str::<std::collections::HashMap<String, String>>(&contents).ok())
        .and_then(|map| map.get("mode").cloned())
        .unwrap_or_else(|| "off".into())
}

/// Rewrites `xdebug.mode=`/`xdebug.output_dir=` in the active `php.ini`,
/// appending the directives if Xdebug's own ini block doesn't declare them
/// yet. Shared by `set_xdebug_mode` and `apply_xdebug_override` (called after
/// a PHP version switch, same pattern as `apply_extension_overrides`).
fn write_xdebug_mode(root: &std::path::Path, mode: &str) -> Result<(), String> {
    let ini = devpanel_php_ini(root)?;
    let original = fs::read_to_string(&ini).map_err(|error| error.to_string())?;
    let output_dir = xdebug_output_dir(root);
    fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;
    let output_dir = output_dir.to_string_lossy().replace('\\', "/");

    let mut saw_mode = false;
    let mut saw_output_dir = false;
    let rewritten = original
        .lines()
        .map(|line| {
            let trimmed = line.trim_start_matches(';').trim();
            if trimmed.to_ascii_lowercase().starts_with("xdebug.mode") {
                saw_mode = true;
                format!("xdebug.mode={mode}")
            } else if trimmed.to_ascii_lowercase().starts_with("xdebug.output_dir") {
                saw_output_dir = true;
                format!("xdebug.output_dir=\"{output_dir}\"")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");

    let mut final_contents = rewritten;
    if !saw_mode {
        final_contents.push_str(&format!("\r\nxdebug.mode={mode}\r\n"));
    }
    if !saw_output_dir {
        final_contents.push_str(&format!("xdebug.output_dir=\"{output_dir}\"\r\n"));
    }
    fs::write(&ini, final_contents).map_err(|e| e.to_string())
}

/// Reapplies the persisted Xdebug mode onto whichever PHP version is
/// currently active — called alongside `apply_extension_overrides` after a
/// PHP version switch.
pub fn apply_xdebug_override(root: &std::path::Path) {
    let mode = load_xdebug_mode(root);
    if mode == "off" {
        return;
    }
    let _ = write_xdebug_mode(root, &mode);
}

#[tauri::command]
pub async fn get_xdebug_mode(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || load_xdebug_mode(&root))
        .await
        .map_err(|error| format!("Xdebug mode task panicked: {error}"))
}

#[tauri::command]
pub async fn set_xdebug_mode(
    mode: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if !XDEBUG_MODES.contains(&mode.as_str()) {
        return Err(format!(
            "Unsupported Xdebug mode '{mode}' — choose one of {XDEBUG_MODES:?}"
        ));
    }
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || {
        let overrides = std::collections::HashMap::from([("mode".to_string(), mode.clone())]);
        let path = xdebug_override_path(&root);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(&overrides).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())?;
        write_xdebug_mode(&root, &mode)
    })
    .await
    .map_err(|error| format!("Xdebug mode task panicked: {error}"))?
}

#[derive(Serialize)]
pub struct XdebugOutputFile {
    pub name: String,
    pub size_bytes: u64,
    pub modified_unix: u64,
}

#[tauri::command]
pub async fn list_xdebug_output(state: tauri::State<'_, AppState>) -> Result<Vec<XdebugOutputFile>, String> {
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || {
        let dir = xdebug_output_dir(&root);
        let mut files = fs::read_dir(&dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                let modified_unix = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                Some(XdebugOutputFile {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    size_bytes: metadata.len(),
                    modified_unix,
                })
            })
            .collect::<Vec<_>>();
        files.sort_by(|a, b| b.modified_unix.cmp(&a.modified_unix));
        Ok(files)
    })
    .await
    .map_err(|error| format!("Xdebug output listing task panicked: {error}"))?
}

#[tauri::command]
pub async fn open_xdebug_output_folder(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let root = state.service_mgr.root().clone();
    let dir = xdebug_output_dir(&root);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().into_owned())
}

fn extension_line_matches(line: &str, file_name: &str) -> Option<bool> {
    let trimmed = line.trim();
    let directive = trimmed.trim_start_matches(';').trim();
    let (_, value) = directive.split_once('=')?;
    if value
        .trim()
        .trim_matches('"')
        .eq_ignore_ascii_case(file_name)
    {
        Some(
            directive
                .to_ascii_lowercase()
                .starts_with("zend_extension="),
        )
    } else {
        None
    }
}

#[tauri::command]
pub async fn get_php_extensions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<PhpExtension>, String> {
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || {
        let ini = devpanel_php_ini(&root)?;
        let contents = fs::read_to_string(&ini).map_err(|error| error.to_string())?;
        let overrides = load_extension_overrides(&root);
        let mut extensions = Vec::new();
        for file_name in php_extension_files(&root)? {
            let mut enabled = false;
            let mut zend_extension = file_name.to_ascii_lowercase().contains("xdebug");
            for line in contents.lines() {
                if let Some(is_zend) = extension_line_matches(line, &file_name) {
                    enabled = !line.trim_start().starts_with(';');
                    zend_extension = is_zend;
                    break;
                }
            }
            let name = extension_name_from_file(&file_name);
            // The stored override (if the user has ever toggled this
            // extension by name) wins over whatever this particular
            // version's php.ini happens to have, so switching PHP versions
            // doesn't look like a random reset in the UI.
            if let Some(&stored) = overrides.get(&name) {
                enabled = stored;
            }
            extensions.push(PhpExtension {
                name,
                file_name,
                enabled,
                zend_extension,
            });
        }
        extensions.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(extensions)
    })
    .await
    .map_err(|error| format!("PHP extensions task panicked: {error}"))?
}

#[tauri::command]
pub async fn set_php_extension(
    file_name: String,
    enabled: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || {
        let files = php_extension_files(&root)?;
        let actual_file = files
            .into_iter()
            .find(|file| file.eq_ignore_ascii_case(&file_name))
            .ok_or_else(|| {
                "That extension DLL is not installed in DevPanel/bin/php/ext.".to_string()
            })?;

        let mut overrides = load_extension_overrides(&root);
        overrides.insert(extension_name_from_file(&actual_file), enabled);
        save_extension_overrides(&root, &overrides)?;

        write_extension_state(&root, &actual_file, enabled)
    })
    .await
    .map_err(|error| format!("PHP extension update task panicked: {error}"))?
}

#[tauri::command]
pub async fn install_xdebug(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || {
        let php = find_binary_in_bin(&root, "php", "php.exe")
            .ok_or_else(|| "PHP is not installed in DevPanel/bin/php.".to_string())?;
        let pie = root.join("bin/pie/pie.phar");
        if !pie.is_file() { return Err("PIE is not installed in DevPanel/bin/pie. Install PIE first; it selects the Xdebug DLL compatible with this PHP build.".into()); }
        let php_dir = php.parent().unwrap_or(&root).to_path_buf();
        let output = Command::new(&php).args([pie.to_string_lossy().as_ref(), "install", "xdebug/xdebug"])
            .current_dir(php_dir).output().map_err(|error| format!("Could not run PIE: {error}"))?;
        let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        if output.status.success() { Ok(text) } else { Err(format!("PIE could not install Xdebug:\n{text}")) }
    }).await.map_err(|error| format!("Xdebug installation task panicked: {error}"))?
}

#[tauri::command]
pub async fn get_site_presets(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SitePresetChoice>, String> {
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || {
        let path = root.join("site-presets.conf");
        let contents = std::fs::read_to_string(&path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        let choices = contents
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    return None;
                }
                let (value, label) = line.split_once('=')?;
                let value = value.trim();
                let label = label.trim();
                if value.is_empty() || label.is_empty() {
                    return None;
                }
                Some(SitePresetChoice {
                    value: value.into(),
                    label: label.into(),
                })
            })
            .collect::<Vec<_>>();
        if choices.is_empty() {
            Err("site-presets.conf contains no valid presets.".into())
        } else {
            Ok(choices)
        }
    })
    .await
    .map_err(|error| format!("Site preset task panicked: {error}"))?
}

#[tauri::command]
pub async fn list_workspaces(state: tauri::State<'_, AppState>) -> Result<Vec<Workspace>, String> {
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    let store = state.workspace_store.lock().await;
    Ok(store.list().into_iter().map(|mut workspace| {
        workspace.path_missing = !scaffold::workspace_path(&root, &www_dir, &workspace).is_dir();
        workspace
    }).collect())
}

/// Finds direct folders under DevPanel's www directory that do not yet have a
/// Site record. This is read-only: registering a project remains an explicit
/// user action so a random folder is never given a domain or vhost silently.
#[tauri::command]
pub async fn discover_workspace_folders(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DiscoveredWorkspaceFolder>, String> {
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    let registered = {
        let store = state.workspace_store.lock().await;
        store.list().into_iter().map(|workspace| workspace.id).collect::<std::collections::HashSet<_>>()
    };
    let base = root.join(www_dir);
    if !base.is_dir() {
        return Ok(Vec::new());
    }
    let mut folders = Vec::new();
    for entry in fs::read_dir(&base).map_err(|error| format!("Could not inspect www folder: {error}"))? {
        let entry = entry.map_err(|error| format!("Could not read www folder entry: {error}"))?;
        if !entry.file_type().map_err(|error| error.to_string())?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if registered.contains(&name) {
            continue;
        }
        let path = entry.path();
        let is_laravel = path.join("artisan").is_file() && path.join("composer.json").is_file();
        folders.push(DiscoveredWorkspaceFolder {
            name,
            path: path.to_string_lossy().into_owned(),
            suggested_preset: if is_laravel { "laravel".into() } else { "php".into() },
            suggested_document_root: if path.join("public").is_dir() { "public".into() } else { String::new() },
        });
    }
    folders.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(folders)
}

#[tauri::command]
pub async fn create_workspace(
    name: String,
    preset: WorkspacePreset,
    options: Option<CreateSiteOptions>,
    state: tauri::State<'_, AppState>,
) -> Result<CreateWorkspaceResult, String> {
    log::info!("Creating workspace '{}' (preset: {:?})", name, preset);
    let mut options = wordpress_options(options);
    let id = scaffold::slugify(&name);
    {
        let store = state.workspace_store.lock().await;
        if store.get(&id).is_some() {
            return Err(format!("A workspace named '{id}' already exists"));
        }
    }

    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    let active_stack = active_stack(&state).await?;
    let is_wordpress = preset.as_str().eq_ignore_ascii_case("wordpress");
    let is_laravel = preset.as_str().eq_ignore_ascii_case("laravel");
    let project_mode = options.project_mode.trim().to_ascii_lowercase();
    let external_root = if project_mode == "existing" {
        let path = PathBuf::from(options.external_root.trim());
        if !path.is_dir() {
            return Err("Choose an existing local folder before creating this site.".into());
        }
        Some(path.canonicalize().map_err(|error| format!("Could not read existing folder: {error}"))?.to_string_lossy().into_owned())
    } else {
        None
    };
    let document_root = options.document_root.trim().trim_matches(['/', '\\']).to_string();
    if PathBuf::from(&document_root).is_absolute() || document_root.split(['/', '\\']).any(|part| part == "..") {
        return Err("Document root must be a relative folder inside the project.".into());
    }
    // Laravel and WordPress use the active DevPanel database by default.
    // A project must never be scaffolded as MariaDB in DevPanel while its
    // framework .env silently remains on the stock SQLite configuration.
    let requires_database = is_wordpress || is_laravel || options.create_database;

    // WordPress is installed during creation and therefore needs a running
    // PHP/database stack. Other project types are deliberately created as
    // stopped sites: choosing a project must not fail just because an
    // unrelated runtime module is not installed or running yet.
    let (ports, mysql_version) = {
        let config = state.config.lock().await;
        (config.get().ports, config.get().mysql_version.clone())
    };
    let mut php_versions = {
        let store = state.workspace_store.lock().await;
        store
            .list()
            .into_iter()
            .map(|workspace| workspace.runtime_profile.php_version)
            .filter(|version| version != "inherit")
            .collect::<Vec<_>>()
    };
    if options.runtime_profile.php_version != "inherit" {
        php_versions.push(options.runtime_profile.php_version.clone());
    }
    if requires_database {
        state
            .service_mgr
            .refresh_services(ports, mysql_version, php_versions)
            .await?;

        for service_id in workspace_service_ids(&active_stack, &options.runtime_profile.php_version, true) {
            state.service_mgr.start(&service_id).await.map_err(|error| {
                format!(
                    "{service_id} could not start. Check that its binary is installed and no other program is using its port.\nDetails: {error}"
                )
            })?;
        }
    }

    // A database is required immediately only for WordPress's guided
    // installer. Commercial packages and custom apps can be created first,
    // then configured when their files and modules are ready.
    let engine_name = requires_database.then(|| stack_database_engine(&active_stack)).transpose()?;
    let (http_port, mysql_port) = {
        let config = state.config.lock().await;
        (
            config.get().ports.public_http_port(&active_stack),
            config.get().ports.mysql,
        )
    };
    if let Some(engine_name) = engine_name.as_ref() {
        let root_for_clone = state.service_mgr.root().clone();
        let engine_name_clone = engine_name.clone();
        tokio::task::spawn_blocking(move || {
            let engine = engine_by_name(&engine_name_clone)?;
            engine.wait_until_ready(&root_for_clone, mysql_port)
        })
        .await
        .map_err(|e| format!("Engine readiness check panicked: {e}"))??;
    }

    let services_started = is_wordpress;
    let active_stack = Some(active_stack);

    let (tld,) = {
        let config = state.config.lock().await;
        (config.get().tld.clone(),)
    };
    let db_name = id.replace('-', "_");
    let domain = format!("{id}{tld}");
    if is_wordpress {
        if options.wordpress_admin_user.trim().is_empty() {
            options.wordpress_admin_user = "admin".into();
        }
        if options.wordpress_admin_password.len() < 8 {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_secs())
                .unwrap_or(0);
            options.wordpress_admin_password = format!("DevPanel!{stamp:x}");
        }
        if !options.wordpress_admin_email.trim().contains('@') {
            options.wordpress_admin_email = format!("admin@{domain}");
        }
    }

    let database_user = format!("dp_{}", id.replace('-', "_"));
    let database_password = format!("dp-{:x}-{:x}", SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_secs()).unwrap_or(0), id.len());
    let database_engine = engine_name.clone().unwrap_or_default();
    let mut workspace = WorkspaceBuilder::new(id.clone(), name, preset, domain, db_name.clone())
        .running(services_started)
        .setup_complete(!is_wordpress)
        .runtime_profile(options.runtime_profile.clone())
        .project_setup(project_mode, external_root, document_root, requires_database)
        .database_binding(database_engine, database_user.clone(), database_password.clone())
        .wordpress_admin(is_wordpress.then(|| WordPressAdmin {
            username: options.wordpress_admin_user.clone(),
            password: options.wordpress_admin_password.clone(),
            email: options.wordpress_admin_email.clone(),
        }))
        .build();

    let ws_for_scaffold = workspace.clone();
    let www_dir_for_scaffold = www_dir.clone();
    let mut warnings = tokio::task::spawn_blocking(move || {
        scaffold::provision(
            &root,
            &www_dir_for_scaffold,
            &ws_for_scaffold,
            active_stack.as_ref(),
            http_port,
        )
    })
    .await
    .map_err(|e| format!("Scaffolding task panicked: {e}"))??;

    // WordPress's guided installer gets a dedicated database now. Other
    // project types can be created without forcing database provisioning.
    if let Some(engine_name) = engine_name.as_ref() {
        let root_for_db = state.service_mgr.root().clone();
        let db_name_for_db = db_name.clone();
        let db_user = database_user.clone();
        let db_pass = database_password.clone();
        let engine_name = engine_name.clone();
        let db_result = tokio::task::spawn_blocking(move || {
            let engine = engine_by_name(&engine_name)?;
            engine.prepare_database(&root_for_db, &db_name_for_db, &db_user, &db_pass)
        })
        .await
        .map_err(|e| format!("Database task panicked: {e}"))?;
        if let Err(error) = db_result {
            warnings.push(format!("Database not created yet: {error}"));
        }
    }

    if is_laravel && workspace.requires_database {
        let root_for_laravel = state.service_mgr.root().clone();
        let www_dir_for_laravel = www_dir.clone();
        let workspace_for_laravel = workspace.clone();
        let laravel_result = tokio::task::spawn_blocking(move || {
            sync_laravel_env_to_binding(&root_for_laravel, &www_dir_for_laravel, &workspace_for_laravel, mysql_port)
        })
        .await
        .map_err(|error| format!("Laravel environment task panicked: {error}"))?;
        if let Err(error) = laravel_result {
            warnings.push(format!("Laravel database binding needs attention: {error}"));
        }
    }

    // One code path for WordPress regardless of engine.
    if workspace.preset.as_str().eq_ignore_ascii_case("wordpress") {
        let root_for_wp = state.service_mgr.root().clone();
        let project_dir = scaffold::project_path(&root_for_wp, &www_dir, &workspace.id);
        let workspace_for_wp = workspace.clone();
        let options_for_wp = options.clone();
        let engine_name = engine_name.expect("WordPress creation requires a database engine");
        let wp_result = tokio::task::spawn_blocking(move || {
            let engine = engine_by_name(&engine_name)?;
            install_wordpress(
                &root_for_wp,
                &project_dir,
                &workspace_for_wp,
                &options_for_wp,
                engine.as_ref(),
                &database_user,
                &database_password,
                mysql_port,
            )
        })
        .await
        .map_err(|error| format!("WordPress setup task panicked: {error}"))?;
        match wp_result {
            Ok(()) => workspace.setup_complete = true,
            Err(error) => warnings.push(format!("WordPress setup needs attention: {error}")),
        }
    }

    let mut store = state.workspace_store.lock().await;
    store.add(workspace.clone())?;
    drop(store);

    match crate::commands::ssl_commands::finish_domain_setup(workspace.id.clone(), state.clone())
        .await
    {
        Ok(setup_warnings) => warnings.extend(setup_warnings),
        Err(error) => warnings.push(format!("HTTPS setup needs attention: {error}")),
    }

    {
        let store = state.workspace_store.lock().await;
        if let Some(updated) = store.get(&workspace.id) {
            workspace = updated;
        }
    }

    Ok(CreateWorkspaceResult {
        workspace,
        warnings,
    })
}

#[tauri::command]
pub async fn get_runtime_catalog(
    state: tauri::State<'_, AppState>,
) -> Result<RuntimeCatalog, String> {
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || {
        let versioned = |folder: &str, executable: &str| runtime_choices(&root, folder, executable);
        Ok(RuntimeCatalog {
            php_versions: versioned("php", "php.exe"),
        })
    })
    .await
    .map_err(|error| format!("Runtime catalog task panicked: {error}"))?
}

/// Re-reads DevPanel's own `bin/` directory. This command has no install-time
/// cache: binaries copied or removed by the user become available immediately.
#[tauri::command]
pub async fn refresh_runtime_detection(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let (ports, mysql_version) = {
        let config = state.config.lock().await;
        (config.get().ports, config.get().mysql_version.clone())
    };
    let php_versions = {
        let store = state.workspace_store.lock().await;
        store
            .list()
            .into_iter()
            .map(|workspace| workspace.runtime_profile.php_version)
            .filter(|version| version != "inherit")
            .collect()
    };
    state
        .service_mgr
        .refresh_services(ports, mysql_version, php_versions)
        .await
}

fn runtime_choices(root: &std::path::Path, folder: &str, executable: &str) -> Vec<RuntimeChoice> {
    let base = root.join("bin").join(folder);
    let mut choices = fs::read_dir(&base)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let is_directory = entry.file_type().ok()?.is_dir();
            if !is_directory {
                return None;
            }
            let path = entry.path();
            let installed = [path.join(executable), path.join("bin").join(executable)]
                .into_iter()
                .any(|candidate| candidate.is_file());
            installed.then(|| {
                let value = entry.file_name().to_string_lossy().into_owned();
                RuntimeChoice {
                    label: value.clone(),
                    value,
                    installed: true,
                }
            })
        })
        .collect::<Vec<_>>();
    choices.sort_by(|left, right| right.value.cmp(&left.value));
    choices
}

#[tauri::command]
pub async fn set_workspace_runtime_profile(
    id: String,
    profile: SiteRuntimeProfile,
    state: tauri::State<'_, AppState>,
) -> Result<Workspace, String> {
    if profile.php_version.trim().is_empty() {
        return Err("Choose an installed PHP version or inherit the default runtime.".into());
    }
    let mut store = state.workspace_store.lock().await;
    let mut workspace = store
        .get(&id)
        .ok_or_else(|| format!("Workspace '{id}' not found"))?;
    if workspace.is_running() {
        return Err("Stop this site before changing its runtime profile.".into());
    }
    if profile.php_version != "inherit" {
        let available = runtime_choices(state.service_mgr.root(), "php", "php.exe");
        if !available
            .iter()
            .any(|choice| choice.value == profile.php_version)
        {
            return Err("That PHP version is not installed in DevPanel/bin/php.".into());
        }
    }
    workspace.runtime_profile = SiteRuntimeProfile {
        php_version: profile.php_version,
    };
    store.update(workspace.clone())?;
    drop(store);
    refresh_runtime_detection(state.clone()).await?;

    let root = state.service_mgr.root().clone();
    let (www_dir, stack, http_port) = {
        let config = state.config.lock().await;
        let stack = environment::find_stack(
            config
                .get()
                .active_stack_id
                .as_deref()
                .unwrap_or(environment::DEFAULT_STACK_ID),
        )?;
        let http_port = config.get().ports.public_http_port(&stack);
        (
            config.get().www_dir.clone().unwrap_or_else(|| "www".into()),
            stack,
            http_port,
        )
    };
    let workspace_for_manifest = workspace.clone();
    tokio::task::spawn_blocking(move || {
        let metadata_dir = scaffold::metadata_path(&root, &www_dir, &workspace_for_manifest.id);
        let mut manifest = crate::workspace::manifest::WorkspaceManifest::load(&metadata_dir)?;
        manifest.php_version = (!workspace_for_manifest
            .runtime_profile
            .php_version
            .eq_ignore_ascii_case("inherit"))
        .then(|| workspace_for_manifest.runtime_profile.php_version.clone());
        manifest.save(&metadata_dir)?;
        crate::workspace::vhost::regenerate(
            &root,
            &www_dir,
            &workspace_for_manifest,
            &stack,
            http_port,
        )
    })
    .await
    .map_err(|error| format!("Runtime profile update task panicked: {error}"))??;
    Ok(workspace)
}

fn validate_domain(domain: &str) -> Result<String, String> {
    let normalized = domain.trim().trim_matches('.').to_ascii_lowercase();
    if normalized.len() > 253
        || normalized.split('.').count() < 2
        || !normalized
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '.' || character == '-')
        || normalized.split('.').any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-'))
    {
        return Err("Enter a valid local domain, for example my-project.dev.".into());
    }
    Ok(normalized)
}

fn validate_db_name(db_name: &str) -> Result<String, String> {
    let value = db_name.trim().to_ascii_lowercase();
    if value.is_empty() || !value.chars().all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_') {
        return Err("Database names may use lowercase letters, numbers and underscores only.".into());
    }
    Ok(value)
}

#[tauri::command]
pub async fn update_workspace_settings(
    id: String,
    settings: WorkspaceSettingsInput,
    state: tauri::State<'_, AppState>,
) -> Result<UpdateWorkspaceSettingsResult, String> {
    let domain = validate_domain(&settings.domain)?;
    let document_root = settings.document_root.trim().trim_matches(['/', '\\']).to_string();
    if PathBuf::from(&document_root).is_absolute()
        || document_root.split(['/', '\\']).any(|part| part == "..")
    {
        return Err("Document root must be a relative folder inside the project.".into());
    }
    let db_name = validate_db_name(&settings.db_name)?;

    let mut workspace = {
        let store = state.workspace_store.lock().await;
        let workspace = store.get(&id).ok_or_else(|| format!("Workspace '{id}' not found"))?;
        if workspace.is_running() {
            return Err("Stop this site before changing its domain, document root or database binding.".into());
        }
        if store.list().iter().any(|other| other.id != id && other.domain.eq_ignore_ascii_case(&domain)) {
            return Err("Another Site is already using this domain.".into());
        }
        workspace
    };
    let root = state.service_mgr.root().clone();
    let (www_dir, stack, http_port) = {
        let config = state.config.lock().await;
        let stack = environment::find_stack(
            config.get().active_stack_id.as_deref().unwrap_or(environment::DEFAULT_STACK_ID),
        )?;
        (
            config.get().www_dir.clone().unwrap_or_else(|| "www".into()),
            stack.clone(),
            config.get().ports.public_http_port(&stack),
        )
    };
    let source_path = scaffold::workspace_path(&root, &www_dir, &workspace);
    if !source_path.is_dir() {
        return Err("The project folder is missing; restore it before editing this Site.".into());
    }
    if !document_root.is_empty() && !source_path.join(&document_root).is_dir() {
        return Err(format!("Document root does not exist: {}", source_path.join(&document_root).display()));
    }

    let old_domain = workspace.domain.clone();
    let domain_changed = old_domain != domain;
    workspace.domain = domain;
    workspace.document_root = document_root.clone();
    workspace.db_name = db_name;
    workspace.requires_database = true;
    if domain_changed {
        // A certificate belongs to its exact host name. Serve HTTP until the
        // explicit Configure HTTPS action issues a certificate for the new one.
        workspace.https_ready = false;
    }
    let workspace_for_write = workspace.clone();
    let warnings = tokio::task::spawn_blocking(move || -> Result<Vec<String>, String> {
        let metadata_dir = scaffold::metadata_path(&root, &www_dir, &workspace_for_write.id);
        let mut manifest = crate::workspace::manifest::WorkspaceManifest::load(&metadata_dir)?;
        manifest.domain = workspace_for_write.domain.clone();
        manifest.doc_root = workspace_for_write.document_root.clone();
        if domain_changed {
            manifest.ssl_enabled = false;
            manifest.ssl_cert_file = None;
            manifest.ssl_key_file = None;
        }
        manifest.save(&metadata_dir)?;
        vhost::regenerate(&root, &www_dir, &workspace_for_write, &stack, http_port)?;
        let mut warnings = Vec::new();
        if domain_changed {
            if let Err(error) = crate::ssl::hosts::remove_entry(&old_domain) {
                warnings.push(format!("Old hosts entry needs cleanup: {error}"));
            }
            if let Err(error) = crate::ssl::hosts::add_entry(&workspace_for_write.domain) {
                warnings.push(format!("New hosts entry needs setup: {error}"));
            }
        }
        Ok(warnings)
    })
    .await
    .map_err(|error| format!("Site settings update task panicked: {error}"))??;

    let mut store = state.workspace_store.lock().await;
    store.update(workspace.clone())?;
    Ok(UpdateWorkspaceSettingsResult { workspace, warnings })
}

fn env_value(contents: &str, key: &str, fallback: &str) -> String {
    contents
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap_or(fallback)
        .trim_matches('"')
        .to_string()
}

fn update_env_values(contents: &str, values: &[(&str, &str)]) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut output = contents
        .lines()
        .map(|line| {
            for (key, value) in values {
                let prefix = format!("{key}=");
                if line.starts_with(&prefix) {
                    seen.insert(*key);
                    return format!("{key}={value}");
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>();
    for (key, value) in values {
        if !seen.contains(key) {
            output.push(format!("{key}={value}"));
        }
    }
    format!("{}\n", output.join("\n"))
}

fn sync_laravel_env_to_binding(
    root: &std::path::Path,
    www_dir: &str,
    workspace: &Workspace,
    mysql_port: u16,
) -> Result<(), String> {
    let path = scaffold::workspace_path(root, www_dir, workspace).join(".env");
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read Laravel .env: {error}"))?;
    let driver = if workspace.database_engine.is_empty() {
        "mariadb"
    } else {
        workspace.database_engine.as_str()
    };
    let values = [
        ("APP_URL", format!("http://{}", workspace.domain)),
        ("DB_CONNECTION", driver.to_string()),
        ("DB_HOST", "127.0.0.1".into()),
        ("DB_PORT", mysql_port.to_string()),
        ("DB_DATABASE", workspace.db_name.clone()),
        ("DB_USERNAME", workspace.database_username.clone()),
        ("DB_PASSWORD", workspace.database_password.clone()),
    ];
    let borrowed = values
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect::<Vec<_>>();
    fs::write(&path, update_env_values(&contents, &borrowed))
        .map_err(|error| format!("Could not save Laravel .env: {error}"))?;

    // Laravel's stock configuration stores sessions/cache/jobs in the
    // database. Create its tables immediately so a correct DB connection
    // does not simply turn into the next \"sessions table is missing\" error.
    let php = scaffold::find_tool(root, "php", "php.exe")
        .ok_or_else(|| "DevPanel PHP runtime is not installed.".to_string())?;
    let project_dir = scaffold::workspace_path(root, www_dir, workspace);
    let output = Command::new(php)
        .args(["artisan", "migrate", "--force"])
        .current_dir(&project_dir)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("Could not run Laravel migrations: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Laravel migrations failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[tauri::command]
pub async fn provision_workspace_database(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Workspace, String> {
    let mut workspace = state.workspace_store.lock().await.get(&id)
        .ok_or_else(|| format!("Workspace '{id}' not found"))?;
    if workspace.is_running() {
        return Err("Stop this site before provisioning or changing its database.".into());
    }
    let stack = active_stack(&state).await?;
    let engine_name = stack_database_engine(&stack)?;
    let (ports, mysql_version, www_dir) = {
        let config = state.config.lock().await;
        (config.get().ports, config.get().mysql_version.clone(), config.get().www_dir.clone().unwrap_or_else(|| "www".into()))
    };
    state.service_mgr.refresh_services(ports, mysql_version, vec![workspace.runtime_profile.php_version.clone()]).await?;
    if let Some(service_id) = crate::db::migration::db_service_of(&stack) {
        state.service_mgr.start(service_id).await?;
    }
    let mysql_port = ports.mysql;
    let root = state.service_mgr.root().clone();
    let database_user = if workspace.database_username.is_empty() { format!("dp_{}", workspace.id.replace('-', "_")) } else { workspace.database_username.clone() };
    let database_password = if workspace.database_password.is_empty() { format!("dp-{:x}-{:x}", workspace.created_at, workspace.id.len()) } else { workspace.database_password.clone() };
    let db_name = workspace.db_name.clone();
    let engine_for_prepare = engine_name.clone();
    let user_for_prepare = database_user.clone();
    let password_for_prepare = database_password.clone();
    tokio::task::spawn_blocking(move || {
        let engine = engine_by_name(&engine_for_prepare)?;
        engine.wait_until_ready(&root, mysql_port)?;
        engine.prepare_database(&root, &db_name, &user_for_prepare, &password_for_prepare)
    }).await.map_err(|error| format!("Database provisioning task panicked: {error}"))??;

    workspace.requires_database = true;
    workspace.database_engine = engine_name;
    workspace.database_username = database_user;
    workspace.database_password = database_password;
    if workspace.preset.as_str().eq_ignore_ascii_case("laravel") {
        let root = state.service_mgr.root().clone();
        let copy = workspace.clone();
        tokio::task::spawn_blocking(move || sync_laravel_env_to_binding(&root, &www_dir, &copy, mysql_port))
            .await.map_err(|error| format!("Laravel binding task panicked: {error}"))??;
    }
    state.workspace_store.lock().await.update(workspace.clone())?;
    Ok(workspace)
}

#[tauri::command]
pub async fn get_laravel_environment(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<LaravelEnvironment, String> {
    let workspace = state.workspace_store.lock().await.get(&id).ok_or_else(|| format!("Workspace '{id}' not found"))?;
    if !workspace.preset.as_str().eq_ignore_ascii_case("laravel") {
        return Err("This editor is available for Laravel sites only.".into());
    }
    let root = state.service_mgr.root().clone();
    let www_dir = state.config.lock().await.get().www_dir.clone().unwrap_or_else(|| "www".into());
    tokio::task::spawn_blocking(move || {
        let path = scaffold::workspace_path(&root, &www_dir, &workspace).join(".env");
        let contents = fs::read_to_string(&path).map_err(|error| format!("Could not read Laravel .env: {error}"))?;
        Ok(LaravelEnvironment {
            app_url: env_value(&contents, "APP_URL", &format!("http://{}", workspace.domain)),
            db_connection: env_value(&contents, "DB_CONNECTION", "mysql"),
            db_host: env_value(&contents, "DB_HOST", "127.0.0.1"),
            db_port: env_value(&contents, "DB_PORT", "3306"),
            db_database: env_value(&contents, "DB_DATABASE", &workspace.db_name),
            db_username: env_value(&contents, "DB_USERNAME", "root"),
            db_password: env_value(&contents, "DB_PASSWORD", ""),
        })
    })
    .await
    .map_err(|error| format!("Laravel environment read task panicked: {error}"))?
}

#[tauri::command]
pub async fn save_laravel_environment(
    id: String,
    environment: LaravelEnvironment,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let workspace = state.workspace_store.lock().await.get(&id).ok_or_else(|| format!("Workspace '{id}' not found"))?;
    if !workspace.preset.as_str().eq_ignore_ascii_case("laravel") {
        return Err("This editor is available for Laravel sites only.".into());
    }
    if workspace.is_running() {
        return Err("Stop this site before changing Laravel environment settings.".into());
    }
    let root = state.service_mgr.root().clone();
    let www_dir = state.config.lock().await.get().www_dir.clone().unwrap_or_else(|| "www".into());
    tokio::task::spawn_blocking(move || {
        let path = scaffold::workspace_path(&root, &www_dir, &workspace).join(".env");
        let contents = fs::read_to_string(&path).map_err(|error| format!("Could not read Laravel .env: {error}"))?;
        let mut values = vec![
            ("APP_URL", environment.app_url.as_str()),
            ("DB_CONNECTION", environment.db_connection.as_str()),
            ("DB_HOST", environment.db_host.as_str()),
            ("DB_PORT", environment.db_port.as_str()),
            ("DB_DATABASE", environment.db_database.as_str()),
            ("DB_USERNAME", environment.db_username.as_str()),
        ];
        if !environment.db_password.is_empty() {
            values.push(("DB_PASSWORD", environment.db_password.as_str()));
        }
        fs::write(&path, update_env_values(&contents, &values))
            .map_err(|error| format!("Could not save Laravel .env: {error}"))
    })
    .await
    .map_err(|error| format!("Laravel environment save task panicked: {error}"))?
}

#[tauri::command]
pub async fn get_workspace_paths(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<WorkspacePaths, String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    if !scaffold::workspace_path(state.service_mgr.root(), &www_dir, &workspace).is_dir() {
        return Err(
            "The project folder is missing. DevPanel did not remove the Site record; restore the folder or remove this Site from Sites first."
                .into(),
        );
    }
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    let php_ini_path = devpanel_php_ini(&root)
        .ok()
        .map(|path| path.to_string_lossy().into_owned());

    let site_path = scaffold::workspace_path(&root, &www_dir, &workspace);
    let find_first = |paths: Vec<PathBuf>| {
        paths
            .into_iter()
            .find(|path| path.exists())
            .map(|path| path.to_string_lossy().into_owned())
    };

    Ok(WorkspacePaths {
        site_path: site_path.to_string_lossy().into_owned(),
        php_ini_path,
        mysql_data_path: root.join("data/mysql").to_string_lossy().into_owned(),
        composer_config_path: find_first(vec![site_path.join("composer.json")]),
        redis_config_path: find_first(vec![
            root.join("bin/redis/redis.conf"),
            root.join("bin/redis/redis.windows.conf"),
        ]),
        memcached_config_path: find_first(vec![
            root.join("bin/memcached/memcached.conf"),
            root.join("bin/memcached/memcached.ini"),
        ]),
        sendmail_path: find_first(vec![
            root.join("bin/sendmail/mailpit.exe"),
            root.join("bin/sendmail"),
        ]),
        heidisql_available: find_dev_tool(&root, "heidisql", &["heidisql.exe", "HeidiSQL.exe"])
            .is_some(),
        cmder_available: find_dev_tool(&root, "cmder", &["Cmder.exe", "cmder.exe"]).is_some(),
        phpmyadmin_available: root.join("bin/phpmyadmin/index.php").is_file(),
    })
}

fn find_dev_tool(root: &std::path::Path, directory: &str, names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .find_map(|name| find_binary_in_bin(root, directory, name))
}

/// Launches HeidiSQL standalone — used by the Tools "Database GUI Launcher"
/// card, which isn't scoped to any one site. `launch_workspace_tool`'s
/// "heidisql" branch is left as-is for the per-site menu since HeidiSQL
/// doesn't actually take the site path either; this just skips the
/// workspace-id lookup that call doesn't need.
#[tauri::command]
pub async fn launch_heidisql(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || {
        Command::new(
            find_dev_tool(&root, "heidisql", &["heidisql.exe", "HeidiSQL.exe"])
                .ok_or_else(|| "HeidiSQL is not installed in DevPanel/bin/heidisql.".to_string())?,
        )
        .spawn()
        .map_err(|error| format!("Could not start HeidiSQL: {error}"))?;
        Ok(())
    })
    .await
    .map_err(|error| format!("HeidiSQL launch task panicked: {error}"))?
}

/// Starts an allow-listed developer application that is owned by DevPanel's
/// bin directory. Arbitrary paths and arguments are never accepted from the UI.
#[tauri::command]
pub async fn launch_workspace_tool(
    id: String,
    tool: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    let site_path = scaffold::workspace_path(&root, &www_dir, &workspace);

    tokio::task::spawn_blocking(move || {
        let mut command = match tool.as_str() {
            "heidisql" => Command::new(
                find_dev_tool(&root, "heidisql", &["heidisql.exe", "HeidiSQL.exe"]).ok_or_else(
                    || "HeidiSQL is not installed in DevPanel/bin/heidisql.".to_string(),
                )?,
            ),
            "cmder" => {
                let mut cmd = Command::new(
                    find_dev_tool(&root, "cmder", &["Cmder.exe", "cmder.exe"]).ok_or_else(
                        || "Cmder is not installed in DevPanel/bin/cmder.".to_string(),
                    )?,
                );
                cmd.arg("/START").arg(&site_path);
                cmd
            }
            _ => return Err("Unsupported developer tool.".into()),
        };
        command
            .spawn()
            .map_err(|error| format!("Could not start {tool}: {error}"))?;
        Ok(())
    })
    .await
    .map_err(|error| format!("Tool launch task panicked: {error}"))?
}

/// Opens a site folder in a user-facing editor or terminal AI CLI. Unlike
/// runtimes, editors deliberately use the user's installed command because
/// they are personal desktop applications, not DevPanel-managed services.
#[tauri::command]
pub async fn launch_workspace_editor(
    id: String,
    editor: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let www_dir = {
        state
            .config
            .lock()
            .await
            .get()
            .www_dir
            .clone()
            .unwrap_or_else(|| "www".into())
    };
    let site_path = scaffold::workspace_path(state.service_mgr.root(), &www_dir, &workspace);
    let command = match editor.as_str() {
        "vscode" => "code",
        "cursor" => "cursor",
        "sublime" => "subl",
        "claude" => "claude",
        "codex" => "codex",
        _ => return Err("Unsupported editor.".into()),
    };
    let mut process = if matches!(editor.as_str(), "claude" | "codex") {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/C", "start", "", "cmd.exe", "/K", command]);
        cmd
    } else {
        let mut cmd = Command::new(command);
        cmd.arg(&site_path);
        cmd
    };
    process.current_dir(&site_path).spawn().map_err(|error| {
        format!("Could not open {editor}. Install its command-line launcher, then retry: {error}")
    })?;
    Ok(())
}

async fn active_stack(state: &AppState) -> Result<environment::StackDefinition, String> {
    let config = state.config.lock().await;
    let stack_id = config
        .get()
        .active_stack_id
        .as_deref()
        .unwrap_or(environment::DEFAULT_STACK_ID);
    environment::find_stack(stack_id)
}

fn stack_database_engine(stack: &environment::StackDefinition) -> Result<String, String> {
    crate::db::migration::db_service_of(stack)
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "The active stack '{}' does not provide a database service.",
                stack.name
            )
        })
}

fn preset_requires_database(preset: &WorkspacePreset) -> bool {
    matches!(
        preset.as_str().to_ascii_lowercase().as_str(),
        "wordpress" | "laravel" | "blesta" | "whmcs"
    )
}

fn workspace_service_ids(
    stack: &environment::StackDefinition,
    php_version: &str,
    requires_database: bool,
) -> Vec<String> {
    let database_service = crate::db::migration::db_service_of(stack);
    stack
        .services
        .iter()
        .filter(|service_id| requires_database || Some(service_id.as_str()) != database_service)
        .map(|service_id| {
            if service_id == "php" {
                crate::service::php_service_id(php_version)
            } else {
                service_id.clone()
            }
        })
        .collect()
}

/// Timeout per service: give it up to 20 seconds to be running before we
/// give up. This prevents the UI being stuck at "Starting…" forever when
/// a binary is missing, a config is busted, or a port is already held.
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(20);

/// Starts the selected workspace's shared runtime dependencies with a
/// bounded timeout per service. Every service is polled with short
/// backoff until it confirms `Running`, then the next service starts.
/// If any service fails or times out, all previously-started services
/// are stopped (rollback) and an error is returned — the workspace is
/// never marked running unless every service is healthy.
#[tauri::command]
pub async fn start_workspace(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Workspace, String> {
    log::info!("Starting workspace: {}", id);
    refresh_runtime_detection(state.clone()).await?;
    let stack = active_stack(&state).await?;
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let configured_www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    if !scaffold::workspace_path(state.service_mgr.root(), &configured_www_dir, &workspace).is_dir() {
        return Err(
            "The project folder is missing. DevPanel did not remove the Site record; restore the folder or remove this Site from Sites first."
                .into(),
        );
    }

    let profile = &workspace.runtime_profile;
    if profile.php_version != "inherit"
        && matches!(&stack.web_role, environment::WebRole::Direct(service) if service == "apache")
    {
        return Err(
            "Apache serves PHP in the same process and cannot honor a per-site PHP version. \
             Choose an Nginx stack, or leave this site on the default PHP version."
                .into(),
        );
    }

    // Self-heal a missing cert: the vhost can end up pointing at
    // ssl_cert_file/ssl_key_file that no longer exist (e.g. the CA was
    // regenerated, or the files were cleared some other way) without
    // ssl_enabled ever being toggled off — the web server would then fail
    // to start at all rather than just serving over HTTP.
    {
        let root = state.service_mgr.root().clone();
        let www_dir = {
            let config = state.config.lock().await;
            config.get().www_dir.clone().unwrap_or_else(|| "www".into())
        };
        let project_dir = scaffold::project_path(&root, &www_dir, &id);
        let needs_reissue = tokio::task::spawn_blocking(move || {
            let Ok(manifest) = crate::workspace::manifest::WorkspaceManifest::load(&project_dir)
            else {
                return false;
            };
            manifest.ssl_enabled
                && !manifest
                    .ssl_cert_file
                    .as_deref()
                    .map(|p| std::path::Path::new(p).is_file())
                    .unwrap_or(false)
        })
        .await
        .unwrap_or(false);
        if needs_reissue {
            log::info!("Reissuing SSL cert for '{id}' — the vhost referenced a missing file");
            let _ =
                crate::commands::ssl_commands::finish_domain_setup(id.clone(), state.clone()).await;
        }
    }

    // Phase 1 — start every service with timeout + polling.
    let mut started_ids: Vec<String> = Vec::new();
    let services = workspace_service_ids(
        &stack,
        &workspace.runtime_profile.php_version,
        workspace.requires_database || preset_requires_database(&workspace.preset),
    );
    for service_id in &services {
        let sid = service_id.clone();
        let result = tokio::time::timeout(SERVICE_START_TIMEOUT, async {
            state.service_mgr.start(&sid).await.map_err(|error| {
                format!("Could not start {sid} for '{}': {error}", workspace.name)
            })?;
            started_ids.push(sid.clone());

            // Poll with increasing intervals (100ms, 200ms, 400ms, 800ms, …)
            // so quick services confirm fast while slow ones still have room.
            let mut delay = Duration::from_millis(100);
            for _ in 0..12 {
                tokio::time::sleep(delay).await;
                let svc_status = state.service_mgr.status(&sid).await;
                match svc_status {
                    ServiceStatus::Running => return Ok::<_, String>(()),
                    ServiceStatus::Error(_) => break,
                    ServiceStatus::Stopped => {}
                }
                delay = (delay * 2).min(Duration::from_secs(2));
            }

            for rolled in started_ids.iter().rev() {
                let _ = state.service_mgr.stop(rolled).await;
            }
            let final_status = state.service_mgr.status(&sid).await;
            Err(format!(
                "{sid} did not reach Running status (last seen: {:?}). \
                 Check its binary, configuration, and port availability.",
                final_status
            ))
        })
        .await;

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                for rolled in started_ids.iter().rev() {
                    let _ = state.service_mgr.stop(rolled).await;
                }
                return Err(e);
            }
            Err(_elapsed) => {
                for rolled in started_ids.iter().rev() {
                    let _ = state.service_mgr.stop(rolled).await;
                }
                return Err(format!(
                    "{sid} did not start within {}s. Check its binary, configuration, and port availability.",
                    SERVICE_START_TIMEOUT.as_secs()
                ));
            }
        }
    }

    // Phase 2 — all services confirmed running, mark workspace as started.
    let mut started = workspace;
    started.start();
    let mut store = state.workspace_store.lock().await;
    store.update(started.clone())?;
    Ok(started)
}

const SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// Immediately marks the workspace as stopped (so the UI updates without
/// waiting), then stops shared services in the background if no other
/// workspace still needs them. If a service times out during shutdown
/// the workspace state is already saved — the user sees "Stopped" right
/// away and a warning is logged instead of blocking the UI.
#[tauri::command]
pub async fn stop_workspace(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Workspace, String> {
    log::info!("Stopping workspace: {}", id);
    let mut stopped = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    stopped.stop();

    let should_stop_services = {
        let mut store = state.workspace_store.lock().await;
        store.update(stopped.clone())?;
        !store.list().iter().any(|workspace| workspace.is_running())
    };

    if should_stop_services {
        let stack = active_stack(&state).await?;
        for service_id in stack.services.iter().rev() {
            let id = service_id.clone();
            let mgr = state.service_mgr.clone();
            tokio::spawn(async move {
                match tokio::time::timeout(SERVICE_STOP_TIMEOUT, mgr.stop(&id)).await {
                    Ok(Ok(_)) => log::info!("Service '{id}' stopped cleanly."),
                    Ok(Err(e)) => log::warn!("Service '{id}' reported error during stop: {e}"),
                    Err(_) => log::warn!(
                        "Service '{id}' did not stop within {}s — it may need manual termination.",
                        SERVICE_STOP_TIMEOUT.as_secs()
                    ),
                }
            });
        }
    }

    Ok(stopped)
}

#[tauri::command]
pub async fn retry_database_setup(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || scaffold::prepare_database(&root, &workspace.db_name))
        .await
        .map_err(|e| format!("Database task panicked: {e}"))?
}

#[tauri::command]
pub async fn delete_workspace_all(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let database_service = active_stack(&state)
        .await
        .ok()
        .and_then(|stack| crate::db::migration::db_service_of(&stack));
    let started_temporarily = if let Some(service_id) = database_service {
        if matches!(
            state.service_mgr.status(service_id).await,
            crate::service::types::ServiceStatus::Running
        ) {
            None
        } else {
            state.service_mgr.start(service_id).await.ok();
            Some(service_id)
        }
    } else {
        None
    };
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    let delete_result =
        tokio::task::spawn_blocking(move || scaffold::delete_all(&root, &www_dir, &workspace))
            .await
            .map_err(|e| format!("Delete task panicked: {e}"))?;
    if let Some(service_id) = started_temporarily {
        let _ = state.service_mgr.stop(service_id).await;
    }
    delete_result?;

    let mut store = state.workspace_store.lock().await;
    store.remove(&id)
}

/// Opens only a known workspace directory through Explorer. Keeping this on
/// the backend avoids the Tauri opener capability and never needs elevation.
#[tauri::command]
pub async fn open_workspace_folder(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        let config = state.config.lock().await;
        config.get().www_dir.clone().unwrap_or_else(|| "www".into())
    };
    let path = scaffold::workspace_path(&root, &www_dir, &workspace);
    if !path.is_dir() {
        return Err(format!(
            "Workspace folder does not exist: {}",
            path.display()
        ));
    }
    Command::new("explorer.exe")
        .arg(path)
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|error| format!("Could not open workspace folder: {error}"))?;
    Ok(())
}

/// Removes the project files and virtual-host configuration but deliberately
/// preserves the database for a later restore or manual migration.
#[tauri::command]
pub async fn uninstall_workspace_keep_data(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let workspace = {
        state
            .workspace_store
            .lock()
            .await
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    let www_dir = {
        state
            .config
            .lock()
            .await
            .get()
            .www_dir
            .clone()
            .unwrap_or_else(|| "www".into())
    };
    tokio::task::spawn_blocking(move || scaffold::uninstall_keep_data(&root, &www_dir, &workspace))
        .await
        .map_err(|error| format!("Uninstall task panicked: {error}"))??;
    state.workspace_store.lock().await.remove(&id)
}

#[tauri::command]
pub async fn delete_workspace_data(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || scaffold::delete_data(&root, &workspace))
        .await
        .map_err(|e| format!("Delete task panicked: {e}"))?
}

#[tauri::command]
pub async fn delete_workspace_config(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let workspace = {
        let store = state.workspace_store.lock().await;
        store
            .get(&id)
            .ok_or_else(|| format!("Workspace '{id}' not found"))?
    };
    let root = state.service_mgr.root().clone();
    tokio::task::spawn_blocking(move || scaffold::delete_config(&root, &workspace))
        .await
        .map_err(|e| format!("Delete task panicked: {e}"))?
}
