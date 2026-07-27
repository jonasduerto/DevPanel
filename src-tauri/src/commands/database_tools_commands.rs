use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::service::find_binary_in_bin;
use crate::service::types::ServiceStatus;
use crate::state::AppState;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize)]
pub struct DatabaseToolStatus {
    pub mysql_installed: bool,
    pub mysql_running: bool,
    pub mysql_versions: Vec<String>,
    pub selected_version: Option<String>,
    pub data_path: String,
}

fn mysql_versions(root: &Path) -> Vec<String> {
    let base = root.join("bin/mysql");
    let mut versions = fs::read_dir(base)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            (path.join("bin/mysqld.exe").is_file() || path.join("mysqld.exe").is_file())
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    versions.sort_by(|a, b| b.cmp(a));
    versions
}

fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(format!(
            "{label} must contain only letters, numbers, and underscores (1–64 characters)."
        ));
    }
    Ok(())
}

fn sql_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "''")
}

fn mysql_client(root: &Path) -> Result<PathBuf, String> {
    find_binary_in_bin(root, "mysql", "mysql.exe")
        .ok_or_else(|| "mysql.exe was not found in DevPanel/bin/mysql.".to_string())
}

fn mysql_command(root: &Path, port: u16, root_password: &str) -> Result<Command, String> {
    let mysql = mysql_client(root)?;
    let mut command = Command::new(mysql);
    command
        .args([
            "--protocol=tcp",
            "-h",
            "127.0.0.1",
            "-P",
            &port.to_string(),
            "-u",
            "root",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null());
    if !root_password.is_empty() {
        // Never put the password in command-line arguments or persist it.
        command.env("MYSQL_PWD", root_password);
    }
    Ok(command)
}

fn execute_sql(root: &Path, port: u16, root_password: &str, sql: &str) -> Result<String, String> {
    let output = mysql_command(root, port, root_password)?
        .arg("-e")
        .arg(sql)
        .output()
        .map_err(|error| format!("Could not run mysql.exe: {error}"))?;
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

async fn mysql_ready(state: &tauri::State<'_, AppState>) -> Result<(), String> {
    if !matches!(
        state.service_mgr.status("mysql").await,
        ServiceStatus::Running
    ) {
        return Err("Start the MySQL service before using Database Tools.".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn get_database_tool_status(
    state: tauri::State<'_, AppState>,
) -> Result<DatabaseToolStatus, String> {
    let root = state.service_mgr.root().clone();
    let selected_version = {
        let config = state.config.lock().await;
        config.get().mysql_version.clone()
    };
    let mysql_running = matches!(
        state.service_mgr.status("mysql").await,
        ServiceStatus::Running
    );
    Ok(DatabaseToolStatus {
        mysql_installed: find_binary_in_bin(&root, "mysql", "mysqld.exe").is_some(),
        mysql_running,
        mysql_versions: mysql_versions(&root),
        selected_version,
        data_path: root.join("data/mysql").to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn set_database_root_password(
    current_password: String,
    new_password: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    mysql_ready(&state).await?;
    let root = state.service_mgr.root().clone();
    let port = { state.config.lock().await.get().ports.mysql };
    let sql = format!(
        "ALTER USER 'root'@'localhost' IDENTIFIED BY '{}'; FLUSH PRIVILEGES;",
        sql_string(&new_password)
    );
    tokio::task::spawn_blocking(move || execute_sql(&root, port, &current_password, &sql))
        .await
        .map_err(|error| format!("Root password task panicked: {error}"))?
        .map(|_| {
            if new_password.is_empty() {
                "Root password removed.".into()
            } else {
                "Root password updated.".into()
            }
        })
}

#[tauri::command]
pub async fn create_database_user(
    database_name: String,
    username: String,
    password: String,
    root_password: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    validate_identifier(&database_name, "Database name")?;
    validate_identifier(&username, "Username")?;
    if username.eq_ignore_ascii_case("root") {
        return Err("The root account cannot be created from this tool.".into());
    }
    if password.is_empty() {
        return Err("A database user password is required.".into());
    }
    mysql_ready(&state).await?;
    let root = state.service_mgr.root().clone();
    let port = { state.config.lock().await.get().ports.mysql };
    let sql = format!(
        "CREATE DATABASE IF NOT EXISTS `{database_name}`; CREATE USER IF NOT EXISTS '{username}'@'localhost' IDENTIFIED BY '{}'; ALTER USER '{username}'@'localhost' IDENTIFIED BY '{}'; GRANT ALL PRIVILEGES ON `{database_name}`.* TO '{username}'@'localhost'; FLUSH PRIVILEGES;",
        sql_string(&password), sql_string(&password)
    );
    tokio::task::spawn_blocking(move || execute_sql(&root, port, &root_password, &sql))
        .await
        .map_err(|error| format!("Create database user task panicked: {error}"))?
        .map(|_| format!("Created database '{database_name}' and user '{username}'@'localhost'."))
}

#[tauri::command]
pub async fn database_repair_all(
    root_password: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    mysql_ready(&state).await?;
    let root = state.service_mgr.root().clone();
    let port = { state.config.lock().await.get().ports.mysql };
    tokio::task::spawn_blocking(move || {
        let mysqlcheck = find_binary_in_bin(&root, "mysql", "mysqlcheck.exe")
            .ok_or_else(|| "mysqlcheck.exe was not found in DevPanel/bin/mysql.".to_string())?;
        let mut command = Command::new(mysqlcheck);
        command
            .args([
                "--protocol=tcp",
                "-h",
                "127.0.0.1",
                "-P",
                &port.to_string(),
                "-u",
                "root",
                "--auto-repair",
                "--all-databases",
            ])
            .creation_flags(CREATE_NO_WINDOW);
        if !root_password.is_empty() {
            command.env("MYSQL_PWD", root_password);
        }
        let output = command
            .output()
            .map_err(|error| format!("Could not run mysqlcheck.exe: {error}"))?;
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
    })
    .await
    .map_err(|error| format!("Database repair task panicked: {error}"))?
}

fn backup_directory(root: &Path) -> PathBuf {
    root.join("data/_database-tools")
}

#[tauri::command]
pub async fn database_backup_all(
    root_password: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    mysql_ready(&state).await?;
    let root = state.service_mgr.root().clone();
    let port = { state.config.lock().await.get().ports.mysql };
    tokio::task::spawn_blocking(move || {
        let mysqldump = find_binary_in_bin(&root, "mysql", "mysqldump.exe")
            .ok_or_else(|| "mysqldump.exe was not found in DevPanel/bin/mysql.".to_string())?;
        let directory = backup_directory(&root);
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs();
        let backup = directory.join(format!("mysql-all-{stamp}.sql"));
        let mut command = Command::new(mysqldump);
        command
            .args([
                "--protocol=tcp",
                "-h",
                "127.0.0.1",
                "-P",
                &port.to_string(),
                "-u",
                "root",
                "--all-databases",
                "--routines",
                "--events",
                "--single-transaction",
            ])
            .creation_flags(CREATE_NO_WINDOW);
        if !root_password.is_empty() {
            command.env("MYSQL_PWD", root_password);
        }
        let output = command
            .output()
            .map_err(|error| format!("Could not run mysqldump.exe: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        fs::write(&backup, output.stdout).map_err(|error| error.to_string())?;
        Ok(backup.to_string_lossy().into_owned())
    })
    .await
    .map_err(|error| format!("Database backup task panicked: {error}"))?
}

#[tauri::command]
pub async fn list_database_backups(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let directory = backup_directory(state.service_mgr.root());
    let mut backups = fs::read_dir(directory)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_name()
                .into_string()
                .ok()
                .filter(|name| name.ends_with(".sql"))
        })
        .collect::<Vec<_>>();
    backups.sort_by(|a, b| b.cmp(a));
    Ok(backups)
}

#[tauri::command]
pub async fn database_restore_backup(
    backup_name: String,
    root_password: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if Path::new(&backup_name)
        .file_name()
        .and_then(|name| name.to_str())
        != Some(backup_name.as_str())
        || !backup_name.ends_with(".sql")
    {
        return Err("Invalid backup selection.".into());
    }
    mysql_ready(&state).await?;
    let root = state.service_mgr.root().clone();
    let port = { state.config.lock().await.get().ports.mysql };
    tokio::task::spawn_blocking(move || {
        let backup = backup_directory(&root).join(&backup_name);
        let contents = fs::read(&backup)
            .map_err(|error| format!("Could not read selected backup: {error}"))?;
        let mut command = mysql_command(&root, port, &root_password)?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| format!("Could not start mysql.exe: {error}"))?;
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .ok_or("Could not open mysql stdin")?
            .write_all(&contents)
            .map_err(|error| error.to_string())?;
        let output = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(format!("Restored {backup_name}."))
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    })
    .await
    .map_err(|error| format!("Database restore task panicked: {error}"))?
}

#[tauri::command]
pub async fn set_database_version(
    version: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if matches!(
        state.service_mgr.status("mysql").await,
        ServiceStatus::Running
    ) {
        return Err("Stop MySQL before selecting a different database version.".into());
    }
    let root = state.service_mgr.root().clone();
    if let Some(value) = &version {
        if !mysql_versions(&root).iter().any(|item| item == value) {
            return Err("That MySQL version is not installed in DevPanel/bin/mysql.".into());
        }
    }
    {
        let mut config = state.config.lock().await;
        config.set_mysql_version(version.clone())?;
    }
    crate::commands::workspace_commands::refresh_runtime_detection(state.clone()).await?;
    Ok(version
        .map(|value| format!("Selected {value}. Start MySQL when ready."))
        .unwrap_or_else(|| "Using the newest detected MySQL version.".into()))
}
